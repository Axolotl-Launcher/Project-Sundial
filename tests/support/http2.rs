// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

pub fn serve_http2(listener: TcpListener, body: [u8; 2]) {
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut preface = [0; 24];
    stream.read_exact(&mut preface).unwrap();
    assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
    stream.write_all(&[0, 0, 0, 4, 0, 0, 0, 0, 0]).unwrap();

    while read_frame(&mut stream) != 1 {}

    // HEADERS carries HPACK's static :status 200 entry. DATA ends stream 1.
    stream
        .write_all(&[
            0, 0, 1, 1, 4, 0, 0, 0, 1, 0x88, 0, 0, 2, 0, 1, 0, 0, 0, 1, body[0], body[1],
        ])
        .unwrap();
    // Let the client finish reading before the server closes the connection.
    let _ = stream.read(&mut preface);
}

fn read_frame(stream: &mut std::net::TcpStream) -> u8 {
    let mut header = [0; 9];
    stream.read_exact(&mut header).unwrap();
    let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    header[3]
}
