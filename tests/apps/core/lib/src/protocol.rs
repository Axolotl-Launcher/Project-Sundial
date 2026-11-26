use super::*;

#[test]
fn reads_each_frame_and_distinguishes_truncation_from_eof() {
    let first = encode(&Input::Request {
        id: 12,
        command: "read",
    })
    .unwrap();
    let second = encode(&Input::<String>::Cancel { id: 12 }).unwrap();
    let frames = [first, second].concat();
    let mut input = frames.as_slice();
    assert!(
        matches!(read::<Input<String>>(&mut input).unwrap(), Some(Input::Request { id: 12, command }) if command == "read")
    );
    assert!(matches!(
        read::<Input<String>>(&mut input).unwrap(),
        Some(Input::Cancel { id: 12 })
    ));
    assert!(read::<Input<String>>(&mut input).unwrap().is_none());
    let mut input = &frames[..3];
    assert_eq!(
        read::<Input<String>>(&mut input).err().unwrap().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[test]
fn rejects_an_oversized_frame_before_reading_it() {
    let header = ((MAX_FRAME + 1) as u32).to_le_bytes();
    assert_eq!(
        read::<Input<()>>(&mut header.as_slice())
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidData
    );
}
