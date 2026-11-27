//! Play-testing teleports, end to end, in a process of their own: the
//! pending request, the arrival's join data and the in-flight flag are
//! process-wide (one Play session in the editor), and the crate's other tests
//! start and stop Play sessions in parallel, which clear them. In the unit
//! test binary that made this test wait out its whole timeout now and then
//! (macOS and Windows CI).

use std::time::{Duration, Instant};

use rustudio_engine::runtime::{RunMode, Side};
use rustudio_player::game::Game;

fn origin(place_id: u64, universe_id: u64) -> rustudio_account::CloudOrigin {
    rustudio_account::CloudOrigin { place_id, universe_id, ..Default::default() }
}

/// A server TeleportAsync to the open place ends the session and starts
/// another on the edit document (unsaved edits included), whose player
/// arrives with the data; Stop then brings the edit document back.
#[test]
fn teleport_to_the_open_place_restarts_play_with_join_data() {
    let mut game = Game::new(None);
    game.doc.cloud = Some(origin(5, 9));
    let edit_id = game.doc.id;
    game.start_play(RunMode::Play, &mut (), false, None);
    assert!(game.is_playing());
    assert_eq!(game.run_luau(Side::Server, r#"
        assert(game.PlaceId==5 and game.GameId==9)
        local p=game:GetService('Players'):GetPlayers()[1]
        assert(next(p:GetJoinData())==nil)
        workspace:SetAttribute('session',1)
        local o=Instance.new('TeleportOptions');o:SetTeleportData({coins=3})
        assert(game:GetService('TeleportService'):TeleportAsync(5,{p},o)==nil)
    "#), None);
    let ctx = egui::Context::default();
    let t = Instant::now();
    let mut ready = false;
    // (generous: a cold CI runner without Studio content is slow to build
    // the destination session)
    while !ready && t.elapsed() < Duration::from_secs(60) {
        game.play_tick(&ctx, 1.0 / 60.0, &mut (), true, false);
        ready = game.poll_teleport();
    }
    assert!(ready, "the destination never got ready");
    game.stop_play(&mut ());
    assert_eq!(game.doc.id, edit_id, "Stop of the old session brings the edit document back");
    game.start_play(RunMode::Play, &mut (), false, None);
    assert_eq!(game.run_luau(Side::Server, r#"
        assert(game.PlaceId==5 and game.GameId==9)
        assert(workspace:GetAttribute('session')==nil,'a fresh server from the edit document')
        local j=game:GetService('Players'):GetPlayers()[1]:GetJoinData()
        assert(j.SourcePlaceId==5 and j.SourceGameId==9 and j.TeleportData.coins==3 and j.Members==nil)
    "#), None);
    assert_eq!(game.run_luau(Side::Client, r#"
        local d=game:GetService('TeleportService'):GetLocalPlayerTeleportData()
        assert(d and d.coins==3)
    "#), None);
    game.stop_play(&mut ());
    assert_eq!(game.doc.id, edit_id);
    // A plain Play afterwards is no teleport's arrival.
    game.start_play(RunMode::Play, &mut (), false, None);
    assert_eq!(game.run_luau(Side::Server, r#"
        assert(next(game:GetService('Players'):GetPlayers()[1]:GetJoinData())==nil)
    "#), None);
    game.stop_play(&mut ());
}
