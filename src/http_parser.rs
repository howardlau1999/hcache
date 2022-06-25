
pub const GET_U8_ARR: [u8; 4] = *b"GET ";
pub const POST_U8_ARR: [u8; 4] = *b"POST";
pub const GET_U32: u32 = unsafe { std::mem::transmute(GET_U8_ARR) };
pub const POST_U32: u32 = unsafe { std::mem::transmute(POST_U8_ARR) };

pub const HTTP_200: &str = "HTTP/1.1 200 OK\r\n\r\n";
pub const HTTP_400: &str = "HTTP/1.1 400 Bad Request\r\n\r\n";
pub const HTTP_404: &str = "HTTP/1.1 404 Not Found\r\n\r\n";