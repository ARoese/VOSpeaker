use std::fmt::Display;

pub const HASH_LEN: usize = 16;
#[derive(Default, Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct ConfigHash(pub [u8; HASH_LEN]);
#[derive(Default, Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct VOHash(pub [u8; HASH_LEN]);

fn string_rep(bytes: &[u8; HASH_LEN]) -> String {
    let hex_string = bytes.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join("");
    hex_string
}
impl Display for ConfigHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", string_rep(&self.0))
    }
}
impl Display for VOHash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", string_rep(&self.0))
    }
}