use crate::Storage;
use monoio::io::{AsyncReadRent, AsyncWriteRentExt};
use std::rc::Rc;
use std::sync::Arc;

pub const HTTP_200: &str = "HTTP/1.1 200 OK\r\n\r\n";
pub const HTTP_400: &str = "HTTP/1.1 400 Bad Request\r\n\r\n";
pub const HTTP_404: &str = "HTTP/1.1 404 Not Found\r\n\r\n";

enum HttpMethod {
    Unknown,
    Get,
    Post,
}

enum HttpRequestParserState {
    ParseMethod,
    SkipBeforePath(usize),
    ParsePath,
    QueryGetKey(usize),
    DelGetKey(usize),
    ZrmvGetKeyValue(usize),
    ReplyInit,
    AddGetBody(usize),
    ListGetBody(usize),
    BatchGetBody(usize),
    AddOrRange,
    ZaddGetKeyAndBody(usize),
    ZrangeGetBody(usize),
    SkipHeaderNoBody(usize),
    ParseBody,
}

struct ParserContext {
    body: Vec<u8>,
    state: HttpRequestParserState,
    method: HttpMethod,
    cur_path_component: Vec<u8>,
    path: Vec<String>,
}

impl ParserContext {
    fn new() -> Self {
        Self {
            body: Vec::new(),
            state: HttpRequestParserState::ParseMethod,
            method: HttpMethod::Unknown,
            path: Vec::new(),
            cur_path_component: Vec::new(),
        }
    }

    async fn feed(self: &mut Self, buf: &[u8], n: usize, stream: &mut monoio::net::TcpStream) {
        let mut buf_pos = 0;
        while buf_pos < buf.len() {
            match self.state {
                HttpRequestParserState::ParseMethod => {
                    if buf[buf_pos] == b'P' {
                        self.method = HttpMethod::Post;
                        self.state = HttpRequestParserState::SkipBeforePath(5);
                    } else {
                        self.method = HttpMethod::Get;
                        self.state = HttpRequestParserState::SkipBeforePath(4);
                    }
                    buf_pos += 1;
                }
                HttpRequestParserState::SkipBeforePath(skip_count) => {
                    let buf_remain = buf.len() - buf_pos;
                    if skip_count <= buf_remain {
                        self.state = HttpRequestParserState::ParsePath;
                        buf_pos += skip_count;
                    } else {
                        self.state =
                            HttpRequestParserState::SkipBeforePath(skip_count - buf_remain);
                        buf_pos = buf.len();
                    }
                }
                HttpRequestParserState::ParsePath => match self.method {
                    HttpMethod::Post => {
                        match buf[buf_pos] {
                            b'l' => {
                                self.state = HttpRequestParserState::ListGetBody(0);
                            }
                            b'b' => {
                                self.state = HttpRequestParserState::BatchGetBody(0);
                            }
                            b'a' => {
                                self.state = HttpRequestParserState::AddGetBody(0);
                            }
                            _ => {
                                self.state = HttpRequestParserState::AddOrRange;
                            }
                        }
                        buf_pos += 1;
                    }
                    _ => {
                        match buf[buf_pos] {
                            b'd' => {
                                self.state = HttpRequestParserState::DelGetKey(3);
                            }
                            b'z' => {
                                self.state = HttpRequestParserState::ZrmvGetKeyValue(4);
                            }
                            _ => {
                                self.state = HttpRequestParserState::ReplyInit;
                            }
                        }
                        buf_pos += 1
                    }
                },
                HttpRequestParserState::AddOrRange => {
                    match buf[buf_pos] {
                        b'a' => {
                            self.state = HttpRequestParserState::AddGetBody(0);
                        }
                        _ => {
                            self.state = HttpRequestParserState::ZaddGetKeyAndBody(4);
                        }
                    }
                    buf_pos += 1;
                }
                _ => {
                    self.state = HttpRequestParserState::SkipHeaderNoBody(0);
                    stream.write_all(HTTP_200.as_bytes()).await.0.unwrap();
                    stream.write_all(b"ok").await.0.unwrap();
                }
            }
        }
    }
}

pub async fn conn_dispatcher(mut stream: monoio::net::TcpStream, _storage: Arc<Storage>) {
    let temp_buffer = [0u8; 4096];
    let mut ctx = ParserContext::new();
    loop {
        let (result, temp_buffer) = stream.read(temp_buffer).await;
        match result {
            Ok(n) => {
                println!("{}", unsafe {
                    String::from_utf8_unchecked(temp_buffer[..n].to_vec())
                });
                ctx.feed(&temp_buffer, n, &mut stream).await;
            }
            Err(e) => {
                println!("Error: {}", e);
                return;
            }
        }
    }
}
