// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

struct CheckOptions(&'static str);

impl Launcher for CheckOptions {
    async fn launch(&self, command: LaunchCommand) -> Result<(), LaunchError> {
        assert_eq!(
            fs::read_to_string(command.working_directory.join("options.txt")).unwrap(),
            self.0,
        );
        Ok(())
    }
}

fn base_version(fixture: &Fixture, release_time: &str) {
    let path = fixture.game_root().join("versions/base/base.json");
    let document = serde_json::json!({"id": "base", "releaseTime": release_time});
    write_file(&path, document.to_string().as_bytes());
}

#[test]
fn the_base_game_date_controls_language_before_the_launcher_runs() {
    for (release, expected) in [
        (
            "2014-06-26T17:00:00+00:00",
            "lang:zh_CN\nforceUnicodeFont:true\n",
        ),
        (
            "2016-08-10T12:30:10+00:00",
            "lang:zh_cn\nforceUnicodeFont:true\n",
        ),
    ] {
        let fixture = Fixture::new();
        base_version(&fixture, release);
        let java = FakeJava::found(fixture.java());
        let mut request = fixture.request();
        request.game_language = Some(GameLanguage::new("base", "zh-CN"));

        preview(&java, &request).unwrap();
        assert!(!request.instance_directory.join("options.txt").exists());
        finish(launch(&java, &CheckOptions(expected), request)).unwrap();
    }
}

#[test]
fn an_existing_world_keeps_its_language_and_unrelated_options() {
    let fixture = Fixture::new();
    base_version(&fixture, "2016-08-10T12:30:10+00:00");
    let java = FakeJava::found(fixture.java());
    let mut request = fixture.request();
    request.game_language = Some(GameLanguage::new("base", "en_us"));
    fs::create_dir_all(request.instance_directory.join("saves")).unwrap();
    write_file(
        &request.instance_directory.join("options.txt"),
        b"fullscreen:false\r\nlang:ja_JP\r\nforceUnicodeFont:true\r\n",
    );

    let expected = "fullscreen:false\r\nlang:ja_jp\r\nforceUnicodeFont:true\r\n";
    finish(launch(&java, &CheckOptions(expected), request)).unwrap();
}

#[test]
fn a_language_selected_without_saves_survives_repeated_launches() {
    let fixture = Fixture::new();
    base_version(&fixture, "2018-07-18T15:11:44+00:00");
    let java = FakeJava::found(fixture.java());
    let mut request = fixture.request();
    request.game_language = Some(GameLanguage::new("base", "en_us"));
    finish(launch(
        &java,
        &CheckOptions("lang:en_us\n"),
        request.clone(),
    ))
    .unwrap();
    write_file(
        &request.instance_directory.join("options.txt"),
        b"lang:zh_cn\nforceUnicodeFont:true\nfullscreen:false",
    );

    let expected = "lang:zh_cn\nforceUnicodeFont:true\nfullscreen:false";
    finish(launch(&java, &CheckOptions(expected), request.clone())).unwrap();
    finish(launch(&java, &CheckOptions(expected), request)).unwrap();
}

#[test]
fn a_malformed_language_preference_does_not_block_launch() {
    let fixture = Fixture::new();
    base_version(&fixture, "2016-08-10T12:30:10+00:00");
    let java = FakeJava::found(fixture.java());
    let mut request = fixture.request();
    request.game_language = Some(GameLanguage::new("base", "invalid:code"));
    let launcher = Recorder::default();

    finish(launch(&java, &launcher, request)).unwrap();
    assert_eq!(launcher.commands().len(), 1);
}

#[test]
fn games_before_language_selection_leave_the_file_absent() {
    let fixture = Fixture::new();
    base_version(&fixture, "2011-11-17T22:00:00+00:00");
    let java = FakeJava::found(fixture.java());
    let mut request = fixture.request();
    request.game_language = Some(GameLanguage::new("base", "zh_cn"));
    let path = request.instance_directory.join("options.txt");

    finish(launch(&java, &Recorder::default(), request)).unwrap();
    assert!(!path.exists());
}

#[test]
fn unreadable_options_stop_the_game_before_spawn() {
    let fixture = Fixture::new();
    base_version(&fixture, "2016-08-10T12:30:10+00:00");
    let java = FakeJava::found(fixture.java());
    let mut request = fixture.request();
    request.game_language = Some(GameLanguage::new("base", "zh_cn"));
    let path = request.instance_directory.join("options.txt");
    write_file(&path, b"\xff\xfe\x6c\x00");
    let launcher = Recorder::default();

    let error = finish(launch(&java, &launcher, request)).unwrap_err();
    assert!(matches!(error, FlowError::GameOptions { .. }));
    assert!(launcher.commands().is_empty());
    assert_eq!(fs::read(path).unwrap(), b"\xff\xfe\x6c\x00");
}
