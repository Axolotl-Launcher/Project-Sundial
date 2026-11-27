// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use axlauncher_network_spi::ServiceHost;

#[test]
fn fallen_proxy_jvm_arguments_apply_only_to_opted_in_microsoft_sessions() {
    let fixture = Fixture::new();
    let proxy = ServiceHost::parse("proxy.example.invalid").unwrap();
    let java = FakeJava::found(fixture.java());
    let microsoft = Session::microsoft_preview("OnlinePlayer", "profile-uuid");
    let offline = Session::offline("OfflinePlayer");
    let third_party = Session::yggdrasil_preview(
        "ExternalPlayer",
        "profile-uuid",
        plan::YggdrasilAgent::new(
            fixture.game_root().join("agent.jar"),
            "https://example.test",
        ),
    );
    for session in [
        microsoft.clone(),
        offline.with_fallen_proxy(Some(proxy.clone())),
        third_party.with_fallen_proxy(Some(proxy.clone())),
    ] {
        let command = preview_with_session(&java, &fixture.request(), &session).unwrap();
        assert!(
            !command
                .args
                .iter()
                .any(|arg| arg.starts_with("-Dminecraft.api."))
        );
    }

    let command = preview_with_session(
        &java,
        &fixture.request(),
        &microsoft.with_fallen_proxy(Some(proxy)),
    )
    .unwrap();
    let main = command
        .args
        .iter()
        .position(|arg| arg == MAIN_CLASS)
        .unwrap();
    let proxy_arguments = command.args[..main]
        .iter()
        .filter(|arg| arg.starts_with("-Dminecraft.api."))
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        proxy_arguments,
        [
            "-Dminecraft.api.env=custom",
            "-Dminecraft.api.auth.host=https://auth.proxy.example.invalid",
            "-Dminecraft.api.account.host=https://account.proxy.example.invalid",
            "-Dminecraft.api.session.host=https://session.proxy.example.invalid",
            "-Dminecraft.api.services.host=https://services.proxy.example.invalid",
            "-Dminecraft.api.profiles.host=https://profiles.proxy.example.invalid",
            "-Dminecraft.api.discovery.host=https://discovery.proxy.example.invalid/minecraft/client",
        ]
    );
}

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
            .any(|argument| argument == &format!("--legacy-session=token:{token}:profile-uuid"))
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

#[test]
fn positional_legacy_manifest_gets_its_session_and_classpath() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    write_file(
        &fixture.version_document(),
        br#"{
        "id":"1.21","mainClass":"net.minecraft.launchwrapper.Launch","libraries":[],
        "minecraftArguments":"${auth_player_name} ${auth_session} --gameDir ${game_directory}"
    }"#,
    );
    let request = fixture.request();
    let preview = preview_with_session(&java, &request, &Session::offline("OldPlayer")).unwrap();
    let entry = preview
        .args
        .iter()
        .position(|arg| arg == "net.minecraft.launchwrapper.Launch")
        .unwrap();
    let before = &preview.args[..entry];
    assert!(
        before
            .windows(2)
            .any(|pair| pair[0] == "-cp" && pair[1].ends_with("1.21.jar"))
    );
    assert!(
        before
            .iter()
            .any(|arg| arg.starts_with("-Djava.library.path="))
    );
    assert_eq!(&preview.args[entry + 1..entry + 3], ["OldPlayer", "-"]);
}

#[test]
fn modern_manifest_receives_only_available_account_fields() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    write_file(&fixture.version_document(), br#"{
        "id":"1.21","mainClass":"net.minecraft.client.main.Main","libraries":[],
        "arguments":{"game":["--username","${auth_player_name}","--uuid","${auth_uuid}",
            "--accessToken","${auth_access_token}","--clientId","${clientid}","--xuid","${auth_xuid}"]}
    }"#);
    let request = fixture.request();
    let session = Session::microsoft("ModernPlayer", "profile-uuid", "token-value".into())
        .with_xuid(Some("2533274790000000".to_owned()));
    let command = preview_with_session(&java, &request, &session).unwrap();
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair == ["--xuid", "2533274790000000"])
    );
    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair == ["--accessToken", "[redacted]"])
    );
    assert!(
        !command
            .args
            .iter()
            .any(|arg| arg == "--clientId" || arg == "--userType")
    );
    let offline = preview_with_session(
        &java,
        &request,
        &Session::offline_profile("OfflinePlayer", "offline-uuid"),
    )
    .unwrap();
    assert!(!offline.args.iter().any(|arg| arg == "--xuid"));
    assert!(
        offline
            .args
            .windows(2)
            .any(|pair| pair == ["--accessToken", "0"])
    );
}

#[test]
fn normal_launches_skip_the_manifest_demo_flag_for_every_account_type() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    write_file(&fixture.version_document(), br#"{
        "id":"1.21","mainClass":"net.minecraft.client.main.Main","libraries":[],
        "arguments":{"game":["--username","${auth_player_name}","--accessToken","${auth_access_token}",
            {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"},
            {"rules":[{"action":"allow","features":{"is_demo_user":false}}],"value":"--normal-feature"}]}
    }"#);
    let agent = plan::YggdrasilAgent::new(
        fixture.game_root().join("agent.jar"),
        "https://example.test/api",
    );
    let sessions = [
        Session::offline_profile("OfflinePlayer", "offline-uuid"),
        Session::microsoft("OnlinePlayer", "online-uuid", "test-token".into()),
        Session::yggdrasil(
            "ExternalPlayer",
            "external-uuid",
            "test-token".into(),
            agent,
        ),
    ];
    for session in sessions {
        let command = preview_with_session(&java, &fixture.request(), &session).unwrap();
        assert!(!command.args.iter().any(|arg| arg == "--demo"));
        assert!(command.args.iter().any(|arg| arg == "--normal-feature"));
    }
}

#[test]
fn flat_named_arguments_drop_unavailable_fields_without_leaving_placeholders() {
    let fixture = Fixture::new();
    let java = FakeJava::found(fixture.java());
    write_file(&fixture.version_document(), br#"{
        "id":"1.21","mainClass":"net.minecraft.client.main.Main","libraries":[],
        "minecraftArguments":"--username ${auth_player_name} --session ${auth_session} --clientId ${clientid} --xuid ${auth_xuid}"
    }"#);
    let session = Session::microsoft("LegacyPlayer", "profile-uuid", "token-value".into());
    let command = preview_with_session(&java, &fixture.request(), &session).unwrap();
    assert!(command.args.ends_with(&[
        "--username".into(),
        "LegacyPlayer".into(),
        "--session".into(),
        "token:[redacted]:profile-uuid".into()
    ]));
}
