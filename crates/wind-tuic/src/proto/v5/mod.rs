mod header;

pub use header::*;

mod cmd;
pub use cmd::*;

mod addr;
pub use addr::*;

const VER: u8 = 5;

#[test]
fn test() {
    use bytes::{Buf as _, BufMut as _, BytesMut};
    let mut bytes = BytesMut::with_capacity(64);
    bytes.put_slice(b"daadadwadwad");
    assert_eq!(bytes.len(), {
        bytes.advance(3);
        bytes.len() + 3
    })
}
