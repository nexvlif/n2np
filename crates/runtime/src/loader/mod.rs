pub struct PeImage {
    pub data: Vec<u8>,
}

impl PeImage {
    pub fn load(_path: &str) -> std::io::Result<Self> {
        Ok(PeImage { data: vec![] })
    }
}