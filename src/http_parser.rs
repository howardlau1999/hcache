
const GET_U8_ARR: [u8; 4] = *b"GET ";
const POST_U8_ARR: [u8; 4] = *b"POST";
const GET_U32: u32 = unsafe { std::mem::transmute(GET_U8_ARR) };
const POST_U32: u32 = unsafe { std::mem::transmute(POST_U8_ARR) };