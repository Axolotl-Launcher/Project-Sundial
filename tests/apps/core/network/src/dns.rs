// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use hickory_proto::rr::{
    Record,
    rdata::{A, AAAA, CNAME},
};

fn name(value: &str) -> Name {
    Name::from_ascii(value).unwrap()
}

fn response(question: &Query) -> Message {
    let mut response = Message::new(0, MessageType::Response, OpCode::Query);
    response.add_query(question.clone());
    response
}

#[test]
fn local_suffixes_match_domain_boundaries() {
    let resolver = DohResolver::new(DohConfig {
        upstreams: Vec::new(),
        system_suffixes: vec![".Private.Example.".into()],
    })
    .unwrap();

    assert!(!resolver.0.uses_system_dns("router"));
    assert!(resolver.0.uses_system_dns("private.example"));
    assert!(resolver.0.uses_system_dns("host.private.example"));
    assert!(!resolver.0.uses_system_dns("unprivate.example"));
    assert!(!resolver.0.uses_system_dns("private.example.com"));
}

#[test]
fn aliases_resolve_to_the_right_address_family_with_the_shortest_ttl() {
    let domain = name("test.example.");
    let alias = name("other.example.");
    let question = Query::query(domain.clone(), RecordType::A);
    let mut response = response(&question);
    response.answers.extend([
        Record::from_rdata(domain, 15, RData::CNAME(CNAME(alias.clone()))),
        Record::from_rdata(
            alias.clone(),
            300,
            RData::A(A("192.0.2.17".parse().unwrap())),
        ),
        Record::from_rdata(alias, 2, RData::AAAA(AAAA("2001:db8::1".parse().unwrap()))),
        Record::from_rdata(
            name("unrelated.example."),
            1,
            RData::A(A("192.0.2.99".parse().unwrap())),
        ),
    ]);

    let answer = decode(response, &question).unwrap();

    assert_eq!(
        answer.addresses,
        ["192.0.2.17:0".parse::<SocketAddr>().unwrap()]
    );
    assert_eq!(answer.ttl, 15);
}

#[test]
fn an_error_response_cannot_win_a_race() {
    let domain = name("test.example.");
    let question = Query::query(domain.clone(), RecordType::A);
    let mut response = response(&question);
    response.metadata.response_code = ResponseCode::ServFail;
    response.answers.push(Record::from_rdata(
        domain,
        15,
        RData::A(A("192.0.2.17".parse().unwrap())),
    ));

    assert!(decode(response, &question).is_none());
}
