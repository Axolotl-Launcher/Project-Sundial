// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn microsoft_token_reaches_only_the_launcher() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    let request = fixture.request();
    let launcher = Recorder::default();
    let token = "private-test-token";
    let session = Session::microsoft("OnlinePlayer", "profile-uuid", token.to_owned());

    let preview = preview_with_session(&java, &request, &session).expect("preview");
    let outcome = finish(launch_with_session(&java, &launcher, request, &session)).expect("launch");
    let launched = &launcher.commands()[0];
    assert!(launched.args.iter().any(|argument| argument == token));
    assert!(
        launched
            .args
            .iter()
            .any(|argument| argument == &format!("--legacy-session={token}"))
    );
    for expected in ["OnlinePlayer", "profile-uuid", "msa"] {
        assert!(launched.args.iter().any(|argument| argument == expected));
    }
    assert!(!format!("{launched:?}").contains(token));
    assert!(!preview.args.join(" ").contains(token));
    assert_eq!(preview, outcome.command);
}

#[test]
fn third_party_agent_precedes_the_main_class_and_keeps_the_token_out_of_diagnostics() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    let request = fixture.request();
    let launcher = Recorder::default();
    let root = "https://example.test/api/yggdrasil";
    let jar = fixture.game_root().join("shared agent.jar");
    let agent =
        plan::YggdrasilAgent::new(jar.clone(), root).with_metadata("public-metadata".into());
    let session = Session::yggdrasil(
        "ExamplePlayer",
        "profile-uuid",
        "third-party-token".into(),
        agent,
    );
    let outcome = finish(launch_with_session(
        &java,
        &launcher,
        request.clone(),
        &session,
    ))
    .unwrap();
    let commands = launcher.commands();
    let args = &commands[0].args;
    let entry = args
        .iter()
        .position(|argument| argument == MAIN_CLASS)
        .unwrap();
    assert!(args[..entry].contains(&format!("-javaagent:{}={root}", jar.display())));
    assert!(args[..entry].contains(&"-Dauthlibinjector.side=client".into()));
    assert!(args[entry..].contains(&"third-party-token".into()));
    assert!(args[entry..].contains(&"legacy".into()));
    assert!(!outcome.command.args.join(" ").contains("third-party-token"));
    assert!(
        !preview(&java, &request)
            .unwrap()
            .args
            .iter()
            .any(|argument| argument.starts_with("-javaagent:"))
    );
}
