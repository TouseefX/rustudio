//! The motion sensors behind UserInputService's AccelerometerEnabled /
//! GyroscopeEnabled, GetDeviceAcceleration / GetDeviceGravity /
//! GetDeviceRotation and the Device*Changed events: Android's
//! `ASensorManager` through ndk-sys (the `ndk` crate has no sensor module),
//! read on a thread of their own with an `ALooper`, so the UI thread only
//! ever copies the latest sample out of a mutex.
//!
//! Units are Android's: m/s² for acceleration and gravity, rad/s for the
//! gyroscope, a unit quaternion for the rotation. The Roblox bridge (the
//! UserInputService device code) scales them to Roblox's (g, and a CFrame).

use std::ffi::{c_int, c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ndk_sys as ffi;

const TYPE_ACCELEROMETER: c_int = 1;
const TYPE_GYROSCOPE: c_int = 4;
const TYPE_GRAVITY: c_int = 9;
const TYPE_LINEAR_ACCELERATION: c_int = 10;
const TYPE_ROTATION_VECTOR: c_int = 11;
const TYPE_GAME_ROTATION_VECTOR: c_int = 15;

/// Roblox fires the device events at the engine's frame rate at most;
/// 60 Hz samples are plenty.
const RATE_USEC: i32 = 16_666;

/// The latest sample of each sensor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// TYPE_LINEAR_ACCELERATION (the accelerometer without gravity), m/s².
    pub acceleration: [f32; 3],
    /// TYPE_GRAVITY, m/s² (about 9.81 long, pointing down in device axes).
    pub gravity: [f32; 3],
    /// TYPE_GYROSCOPE, rad/s about the device axes.
    pub gyro: [f32; 3],
    /// TYPE_GAME_ROTATION_VECTOR (else TYPE_ROTATION_VECTOR): the device's
    /// orientation as a unit quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    /// How many samples have arrived (a changed count means a changed
    /// reading, for firing the *Changed events once per new sample).
    pub acceleration_seq: u32,
    pub gravity_seq: u32,
    pub gyro_seq: u32,
    pub rotation_seq: u32,
}

/// What the device has, asked once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Available {
    pub accelerometer: bool,
    pub gyroscope: bool,
}

struct Shared {
    reading: Mutex<Reading>,
    want_accel: AtomicBool,
    want_gyro: AtomicBool,
    /// Bumped whenever the wanted set changes; the thread re-applies it.
    generation: AtomicU32,
    available: OnceLock<Available>,
}

static SHARED: Shared = Shared {
    reading: Mutex::new(Reading {
        acceleration: [0.0; 3],
        gravity: [0.0; 3],
        gyro: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        acceleration_seq: 0,
        gravity_seq: 0,
        gyro_seq: 0,
        rotation_seq: 0,
    }),
    want_accel: AtomicBool::new(false),
    want_gyro: AtomicBool::new(false),
    generation: AtomicU32::new(0),
    available: OnceLock::new(),
};

static THREAD: OnceLock<()> = OnceLock::new();

/// A pointer the sensor thread may own (ASensorManager is process-global
/// and thread-safe; the queue is only touched on its thread).
struct Manager(*mut ffi::ASensorManager);
unsafe impl Send for Manager {}

fn manager() -> Option<Manager> {
    // ASensorManager_getInstanceForPackage (API 26) wants the package name;
    // an empty one gets the process-wide manager, as the older
    // ASensorManager_getInstance did.
    let package = package_name().and_then(|p| CString::new(p).ok()).unwrap_or_default();
    // SAFETY: a valid C string; the manager lives as long as the process.
    let m = unsafe { ffi::ASensorManager_getInstanceForPackage(package.as_ptr()) };
    (!m.is_null()).then_some(Manager(m))
}

fn package_name() -> Option<String> {
    super::jni::with_activity(|env, activity| {
        use jni::{jni_sig, jni_str};
        let s = env
            .call_method(activity, jni_str!("getPackageName"), jni_sig!("()Ljava/lang/String;"), &[])?
            .l()?;
        let s = env.as_cast::<jni::objects::JString>(&s)?;
        Ok((*s).to_string())
    })
    .ok()
}

/// Which sensors this device has (the UserInputService properties say
/// "enabled" only when there is something to enable).
pub fn available() -> Available {
    *SHARED.available.get_or_init(|| {
        let Some(m) = manager() else {
            return Available::default();
        };
        // SAFETY: a valid manager; getDefaultSensor returns null for a type
        // the device lacks.
        let has = |t: c_int| unsafe { !ffi::ASensorManager_getDefaultSensor(m.0, t).is_null() };
        Available { accelerometer: has(TYPE_ACCELEROMETER), gyroscope: has(TYPE_GYROSCOPE) }
    })
}

/// Turns the accelerometer group (accelerometer, gravity, linear
/// acceleration) and the gyroscope group (gyroscope, rotation vector) on
/// or off. The thread starts on the first enable.
pub fn set_enabled(accelerometer: bool, gyroscope: bool) {
    let before = (SHARED.want_accel.load(Ordering::Relaxed), SHARED.want_gyro.load(Ordering::Relaxed));
    SHARED.want_accel.store(accelerometer, Ordering::Relaxed);
    SHARED.want_gyro.store(gyroscope, Ordering::Relaxed);
    if before != (accelerometer, gyroscope) {
        SHARED.generation.fetch_add(1, Ordering::Release);
    }
    if (accelerometer || gyroscope) && THREAD.get().is_none() {
        THREAD.get_or_init(|| {
            if let Err(e) = std::thread::Builder::new().name("sensors".into()).spawn(run) {
                log::error!("sensors thread: {e}");
            }
        });
    }
}

/// The latest samples.
pub fn latest() -> Reading {
    *SHARED.reading.lock().unwrap_or_else(|e| e.into_inner())
}

/// The sensor thread: its own looper and event queue, the wanted sensors
/// enabled, events drained as they come; it idles (polling every 250 ms
/// for a change of mind) while nothing is enabled.
fn run() {
    let Some(m) = manager() else {
        log::warn!("sensors: no ASensorManager");
        return;
    };
    // SAFETY: prepares a looper for this thread (non-callback polling), and
    // makes a queue on it; both are only used from this thread.
    let (looper, queue) = unsafe {
        let looper = ffi::ALooper_prepare(ffi::ALOOPER_PREPARE_ALLOW_NON_CALLBACKS as c_int);
        if looper.is_null() {
            log::warn!("sensors: ALooper_prepare failed");
            return;
        }
        let queue = ffi::ASensorManager_createEventQueue(m.0, looper, 1, None, std::ptr::null_mut());
        if queue.is_null() {
            log::warn!("sensors: no event queue");
            return;
        }
        (looper, queue)
    };
    let _ = looper;
    // SAFETY: a valid manager; null for types the device lacks.
    let sensor = |t: c_int| unsafe {
        let s = ffi::ASensorManager_getDefaultSensor(m.0, t);
        (!s.is_null()).then_some(s)
    };
    let accel_group: Vec<_> = [TYPE_ACCELEROMETER, TYPE_GRAVITY, TYPE_LINEAR_ACCELERATION].into_iter().filter_map(sensor).collect();
    let mut gyro_group: Vec<_> = [TYPE_GYROSCOPE].into_iter().filter_map(sensor).collect();
    // A game rotation vector (no magnetometer: no yaw jumps) when there is
    // one, else the rotation vector.
    if let Some(r) = sensor(TYPE_GAME_ROTATION_VECTOR).or_else(|| sensor(TYPE_ROTATION_VECTOR)) {
        gyro_group.push(r);
    }
    let mut applied = u32::MAX;
    let mut on_accel = false;
    let mut on_gyro = false;
    let mut events: [ffi::ASensorEvent; 16] = [unsafe { std::mem::zeroed() }; 16];
    loop {
        let gen = SHARED.generation.load(Ordering::Acquire);
        if gen != applied {
            applied = gen;
            let want_accel = SHARED.want_accel.load(Ordering::Relaxed);
            let want_gyro = SHARED.want_gyro.load(Ordering::Relaxed);
            for (group, on, want) in [(&accel_group, &mut on_accel, want_accel), (&gyro_group, &mut on_gyro, want_gyro)] {
                if *on == want {
                    continue;
                }
                for &s in group {
                    // SAFETY: a queue and sensor from this manager.
                    unsafe {
                        if want {
                            ffi::ASensorEventQueue_enableSensor(queue, s);
                            ffi::ASensorEventQueue_setEventRate(queue, s, RATE_USEC.max(ffi::ASensor_getMinDelay(s)));
                        } else {
                            ffi::ASensorEventQueue_disableSensor(queue, s);
                        }
                    }
                }
                *on = want;
            }
            log::info!("sensors: accelerometer {on_accel}, gyroscope {on_gyro}");
        }
        if !on_accel && !on_gyro {
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        // SAFETY: a poll on this thread's looper; the out-pointers may be
        // null.
        let ident = unsafe {
            ffi::ALooper_pollOnce(250, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut::<*mut c_void>())
        };
        if ident == ffi::ALOOPER_POLL_ERROR {
            log::warn!("sensors: looper error");
            break;
        }
        loop {
            // SAFETY: a valid queue and a buffer of 16 events.
            let n = unsafe { ffi::ASensorEventQueue_getEvents(queue, events.as_mut_ptr(), events.len()) };
            if n <= 0 {
                break;
            }
            let mut r = SHARED.reading.lock().unwrap_or_else(|e| e.into_inner());
            for ev in &events[..n as usize] {
                // SAFETY: every event type read here fills `data` (the
                // union's largest member) from the front.
                let d = unsafe { ev.__bindgen_anon_1.__bindgen_anon_1.data };
                match ev.type_ {
                    TYPE_LINEAR_ACCELERATION => {
                        r.acceleration = [d[0], d[1], d[2]];
                        r.acceleration_seq = r.acceleration_seq.wrapping_add(1);
                    }
                    TYPE_GRAVITY => {
                        r.gravity = [d[0], d[1], d[2]];
                        r.gravity_seq = r.gravity_seq.wrapping_add(1);
                    }
                    // Without a linear-acceleration sensor the accelerometer
                    // minus the last gravity sample stands in.
                    TYPE_ACCELEROMETER if !accel_group.iter().any(|&s| unsafe { ffi::ASensor_getType(s) } == TYPE_LINEAR_ACCELERATION) => {
                        r.acceleration = [d[0] - r.gravity[0], d[1] - r.gravity[1], d[2] - r.gravity[2]];
                        r.acceleration_seq = r.acceleration_seq.wrapping_add(1);
                    }
                    TYPE_GYROSCOPE => {
                        r.gyro = [d[0], d[1], d[2]];
                        r.gyro_seq = r.gyro_seq.wrapping_add(1);
                    }
                    TYPE_ROTATION_VECTOR | TYPE_GAME_ROTATION_VECTOR => {
                        let (x, y, z) = (d[0], d[1], d[2]);
                        // The fourth component is the scalar part; sensors
                        // that leave it out imply it from the unit length.
                        let w = if d[3] != 0.0 { d[3] } else { (1.0 - (x * x + y * y + z * z)).max(0.0).sqrt() };
                        r.rotation = [x, y, z, w];
                        r.rotation_seq = r.rotation_seq.wrapping_add(1);
                    }
                    _ => {}
                }
            }
        }
    }
    // SAFETY: the queue was made by this manager on this thread.
    unsafe {
        ffi::ASensorManager_destroyEventQueue(m.0, queue);
    }
}
