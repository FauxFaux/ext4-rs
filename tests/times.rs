use ext4::Time;

#[test]
fn future_file() {
    // 2345-06-07 08:09:10.111213141Z
    let time = Time::from_extra(0xc229_d726u32 as i32, Some(0x1a83_e957));
    assert_eq!(11847456550, time.epoch_secs);
    assert_eq!(Some(111213141), time.nanos);
}
