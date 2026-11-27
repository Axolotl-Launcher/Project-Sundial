// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn a_historical_id_with_spaces_previews_and_reaches_the_launcher() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    let mut request = fixture.request();
    let id = "1.14 Pre-Release 1";
    request.version_id = id.to_owned();
    let document = fixture
        .game_root()
        .join("versions")
        .join(id)
        .join(format!("{id}.json"));
    write_file(&document, VERSION_JSON.replace(VERSION_ID, id).as_bytes());
    let jar = axlauncher_storage::GameDir::standalone(fixture.game_root()).client_jar(id);
    write_file(&jar, b"client jar");

    let command = preview(&java, &request).expect("preview keeps the catalog ID");
    assert!(
        command
            .args
            .iter()
            .any(|argument| argument.contains(&*jar.to_string_lossy()))
    );
    let recorder = Recorder::default();
    let outcome =
        finish(launch(&java, &recorder, request)).expect("launch builds the same command");
    assert_eq!(outcome.command, command);
}
