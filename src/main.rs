use md5;
pub fn shorten(url: String) -> String {
    let digest = md5::compute(url);
    return format!("{:x}", digest);
}
