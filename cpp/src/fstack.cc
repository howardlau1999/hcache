#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <folly/io/IOBuf.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unordered_map>

#include <picohttpparser/picohttpparser.h>

#include "ff_api.h"
#include "ff_config.h"

#define MAX_EVENTS 512

/* kevent set */
struct kevent kevSet;
/* events */
struct kevent events[MAX_EVENTS];
/* kq */
int kq;
int sockfd;

char html1[1], html2[1], html[1];
extern int ff_zc_mbuf_get(struct ff_zc_mbuf *m, int len);
extern int ff_zc_mbuf_write(struct ff_zc_mbuf *m, const char *data, int len);

char *buf_tmp;
char html_buf[10240];
size_t buf_len = 0;
struct ff_zc_mbuf zc_buf;

#define COMMON_HEADER "Connection: keep-alive\r\nContent-Length: "

const char HTTP_200[] = "HTTP/1.1 200 OK\r\n" COMMON_HEADER;
const char HTTP_404[] = "HTTP/1.1 404 Not Found\r\n" COMMON_HEADER;
const char HTTP_400[] = "HTTP/1.1 400 Bad Request\r\n" COMMON_HEADER;
const char *CONTENT_LENGTH_FORMAT = "{}\r\n\r\n";
const char INIT_RESPONSE[] = "HTTP/1.1 200 OK\r\nConnection: keep-alive\r\nContent-Length: 2\r\n\r\nok";

enum class CacheMethod {
  Unknown,
  Init,
  Query,
  Add,
  Del,
  Batch,
  List,
  Zadd,
  Zrmv,
  Zrange,
};

struct connection {
  connection(int fd) : fd(fd), recv_buf(folly::IOBuf::create(512)) {}
  std::unique_ptr<folly::IOBuf> recv_buf;
  struct ff_zc_mbuf zc_send_buf;
  CacheMethod func;
  int fd;
  size_t body_read;
  size_t body_len;
};

std::unordered_map<int, std::unique_ptr<connection>> connections;

int loop(void *arg) {
  /* Wait for events to happen */
  unsigned nevents = ff_kevent(kq, NULL, 0, events, MAX_EVENTS, NULL);
  unsigned i;

  buf_len = sizeof(html) - 1;
  buf_tmp = html;

  for (i = 0; i < nevents; ++i) {
    struct kevent event = events[i];
    int clientfd = (int) event.ident;

    /* Handle disconnect */
    if (event.flags & EV_EOF) {
      /* Simply close socket */
      ff_close(clientfd);
    } else if (clientfd == sockfd) {
      int available = (int) event.data;
      do {
        int nclientfd = ff_accept(clientfd, NULL, NULL);
        if (nclientfd < 0) {
          printf("ff_accept failed:%d, %s\n", errno, strerror(errno));
          break;
        }

        /* Add to event list */
        EV_SET(&kevSet, nclientfd, EVFILT_READ, EV_ADD, 0, 0, NULL);
        connections[nclientfd] = std::make_unique<connection>(nclientfd);

        if (ff_kevent(kq, &kevSet, 1, NULL, 0, NULL) < 0) {
          printf("ff_kevent error:%d, %s\n", errno, strerror(errno));
          return -1;
        }

        available--;
      } while (available);
    } else if (event.filter == EVFILT_READ) {
      auto &conn = *connections[clientfd];
      auto room = conn.recv_buf->tailroom();
      if (room < 512) { conn.recv_buf->reserve(0, 512); }
      conn.recv_buf->unshare();
      size_t readlen = ff_recv(clientfd, conn.recv_buf->writableData(), 512, 0);
      if (conn.func == CacheMethod::Unknown) {
        const char *path;
        const char *method;
        int minor_version;
        size_t path_len;
        size_t method_len;
        size_t num_headers;
        phr_header *headers;
        int parser_ret = phr_parse_request(
            reinterpret_cast<char *>(conn.recv_buf->writableData()), conn.recv_buf->length(), &method, &method_len, &path,
            &path_len, &minor_version, headers, &num_headers, 0);
        if (parser_ret == -1) {
          ff_close(clientfd);
          connections.erase(clientfd);
          continue;
        } else if (parser_ret == -2) {
          continue;
        } else {
          if (method[0] == 'G') {
            switch (path[1]) {
              case 'q':
                conn.func = CacheMethod::Query;
                break;
              case 'd':
                conn.func = CacheMethod::Del;
                break;
              case 'z':
                conn.func = CacheMethod::Zrmv;
                break;
              default: {
                int buf_len = sizeof(INIT_RESPONSE);
                int ret = ff_zc_mbuf_get(&zc_buf, buf_len);
                if (ret < 0) {
                  printf("ff_zc_mbuf_get failed, len:%d, errno:%d, %s\n", buf_len, errno, strerror(errno));
                  exit(1);
                }
                ff_zc_mbuf_write(&zc_buf, INIT_RESPONSE, buf_len);
                ff_write(clientfd, zc_buf.bsd_mbuf, buf_len);
              } break;
            }
          } else {
            switch (path[1]) {
              case 'a':
                conn.func = CacheMethod::Add;
                break;
              case 'b':
                conn.func = CacheMethod::Batch;
                break;
              case 'l':
                conn.func = CacheMethod::List;
                break;
              case 'z':
                switch (path[2]) {
                  case 'a':
                    conn.func = CacheMethod::Zadd;
                    break;
                  case 'r':
                    conn.func = CacheMethod::Zrange;
                    break;
                }
                break;
            }
          }
        }
      }
    } else {
      printf("unknown event: %8.8X\n", event.flags);
    }
  }
  return 0;
}

int main(int argc, char *argv[]) {
  ff_init(argc, argv);

  assert((kq = ff_kqueue()) > 0);

  sockfd = ff_socket(AF_INET, SOCK_STREAM, 0);
  if (sockfd < 0) {
    printf("ff_socket failed, sockfd:%d, errno:%d, %s\n", sockfd, errno, strerror(errno));
    exit(1);
  }

  struct sockaddr_in my_addr;
  bzero(&my_addr, sizeof(my_addr));
  my_addr.sin_family = AF_INET;
  my_addr.sin_port = htons(8080);
  my_addr.sin_addr.s_addr = htonl(INADDR_ANY);

  int ret = ff_bind(sockfd, (struct linux_sockaddr *) &my_addr, sizeof(my_addr));
  if (ret < 0) {
    printf("ff_bind failed, sockfd:%d, errno:%d, %s\n", sockfd, errno, strerror(errno));
    exit(1);
  }

  ret = ff_listen(sockfd, MAX_EVENTS);
  if (ret < 0) {
    printf("ff_listen failed, sockfd:%d, errno:%d, %s\n", sockfd, errno, strerror(errno));
    exit(1);
  }

  EV_SET(&kevSet, sockfd, EVFILT_READ, EV_ADD, 0, MAX_EVENTS, NULL);
  /* Update kqueue */
  ff_kevent(kq, &kevSet, 1, NULL, 0, NULL);

  ff_run(loop, NULL);
  return 0;
}