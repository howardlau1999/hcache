#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <folly/FBString.h>
#include <folly/FBVector.h>
#include <folly/io/IOBuf.h>
#include <hcache/data_loader.h>
#include <hcache/storage.h>
#include <rapidjson/document.h>
#include <rapidjson/rapidjson.h>
#include <rapidjson/writer.h>
#include <simdjson.h>
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
storage hcache;

const char HTTP_200[] = "HTTP/1.1 200 OK\r\nContent-Length: ";
const char HTTP_404[] = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
const char HTTP_400[] = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
const char *CONTENT_LENGTH_FORMAT = "%d\r\n\r\n";
const char INIT_RESPONSE[] = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
const char EMPTY_RESPONSE[] = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";

enum class CacheMethod {
  Unknown,
  Init,
  Query,
  Del,
  Zrmv,
  Add,
  Batch,
  List,
  Zadd,
  Zrange,
};

struct connection {
  connection(int fd) : fd(fd), recv_buf(folly::IOBuf::create(512)) {}
  std::unique_ptr<folly::IOBuf> recv_buf;
  struct ff_zc_mbuf zc_send_buf;
  CacheMethod func;
  int fd;
  size_t body_len;
  folly::fbstring zkey;
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
      connections.erase(clientfd);
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
      size_t readlen = ff_recv(clientfd, conn.recv_buf->writableTail(), 512, 0);
      if (readlen == 0) {
        ff_close(clientfd);
        connections.erase(clientfd);
        continue;
      }
      conn.recv_buf->append(readlen);
      if (conn.func == CacheMethod::Unknown) {
        const char *path;
        const char *method;
        int minor_version;
        size_t path_len;
        size_t method_len;
        size_t num_headers;
        phr_header headers[16];
        int parser_ret = phr_parse_request(
            reinterpret_cast<const char *>(conn.recv_buf->data()), conn.recv_buf->length(), &method, &method_len, &path,
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
              case 'q': {
                const char *key = path + 7;
                auto key_len = path_len - 7;
                auto value = hcache.get_value_by_key(folly::fbstring(key, key_len));
                if (value.has_value()) {
                  auto &&value_data = value.value();
                  char len_str_buf[16] = {0};
                  auto len_str_size =
                      snprintf(len_str_buf, sizeof(len_str_buf), CONTENT_LENGTH_FORMAT, value_data.size());
                  ff_send(clientfd, HTTP_200, sizeof(HTTP_200) - 1, 0);
                  ff_send(clientfd, len_str_buf, len_str_size, 0);
                  ff_send(clientfd, value_data.data(), value_data.size(), 0);
                } else {
                  ff_send(clientfd, HTTP_404, sizeof(HTTP_404) - 1, 0);
                }
              } break;
              case 'd': {
                const char *key = path + 5;
                auto key_len = path_len - 5;
                hcache.del_key(folly::fbstring(key, key_len));
                ff_send(clientfd, EMPTY_RESPONSE, sizeof(EMPTY_RESPONSE) - 1, 0);
              } break;
              case 'z': {
                int slash_idx = 6;
                while (slash_idx < path_len && path[slash_idx] != '/') { slash_idx++; }
                hcache.zset_rmv(
                    folly::fbstring(path + 6, slash_idx - 6),
                    folly::fbstring(path + slash_idx + 1, path_len - slash_idx - 1));
                ff_send(clientfd, EMPTY_RESPONSE, sizeof(EMPTY_RESPONSE) - 1, 0);
              } break;
              default: {
                int buf_len = sizeof(INIT_RESPONSE) - 1;
                ff_write(clientfd, INIT_RESPONSE, buf_len);
              } break;
            }
          } else {
            size_t body_len = 0;
            for (size_t i = 0; i < num_headers; ++i) {
              if (strncasecmp(headers[i].name, "Content-Length", headers[i].name_len) == 0) {
                body_len = std::stoi(std::string(headers[i].value, headers[i].value_len));
                break;
              }
            }
            conn.body_len = body_len;
            conn.recv_buf->trimStart(parser_ret);
            if (conn.recv_buf->length() == 0) { conn.recv_buf->clear(); }

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
                conn.zkey = folly::fbstring(path + 6, path_len - 6);
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
      if (conn.func > CacheMethod::Zrmv) {
        if (conn.recv_buf->length() >= conn.body_len) {
          conn.recv_buf->trimStart(conn.body_len);
          if (conn.recv_buf->length() == 0) { conn.recv_buf->clear(); }
          conn.body_len = 0;
          conn.func = CacheMethod::Unknown;
          simdjson::dom::parser parser;
          simdjson::padded_string_view padded = simdjson::padded_string_view(
              reinterpret_cast<const char *>(conn.recv_buf->data()), conn.recv_buf->length());
          auto doc = parser.parse(padded);
          switch (conn.func) {
            case CacheMethod::Add: {
              auto key = doc["key"].get_string().take_value();
              auto value = doc["value"].get_string().take_value();
              hcache.add_key_value(folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), key.size()));
            } break;
            case CacheMethod::Zadd: {
              auto score = doc["score"].get_uint64().take_value();
              auto value = doc["value"].get_string().take_value();
              hcache.zset_add(std::move(conn.zkey), folly::fbstring(value.data(), value.size()), score);
            } break;
            case CacheMethod::Batch: {
              for (auto &&item: doc) {
                auto key = item["key"].get_string().take_value();
                auto value = item["value"].get_string().take_value();
                hcache.add_key_value(
                    folly::fbstring(key.data(), key.size()), folly::fbstring(value.data(), key.size()));
              }
            } break;
            case CacheMethod::List: {
              folly::F14FastSet<folly::StringPiece> keys;
              for (auto &&item: doc) {
                auto key = item["key"].get_string().take_value();
                keys.emplace(key);
              }
              auto result = hcache.list_keys(keys);
              if (result.empty()) {
                ff_send(clientfd, HTTP_404, sizeof(HTTP_404) - 1, 0);
              } else {
                auto &&d = rapidjson::Document();
                auto &kv_list = d.SetArray();
                auto &allocator = d.GetAllocator();
                for (auto &&kv: result) {
                  auto &&kv_object = rapidjson::Value(rapidjson::kObjectType);
                  auto &&k_string = rapidjson::StringRef(kv.key.data(), kv.key.size());
                  auto &&v_string = rapidjson::StringRef(kv.value.data(), kv.value.size());
                  kv_object.AddMember("key", k_string, allocator);
                  kv_object.AddMember("value", v_string, allocator);
                  kv_list.PushBack(kv_object, allocator);
                }
                rapidjson::StringBuffer buffer;
                rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
                d.Accept(writer);
                char len_str[16] = {0};
                auto len_str_size = snprintf(len_str, sizeof(len_str), CONTENT_LENGTH_FORMAT, buffer.GetSize());
                ff_send(clientfd, HTTP_200, sizeof(HTTP_200) - 1, 0);
                ff_send(clientfd, len_str, len_str_size, 0);
                ff_send(clientfd, buffer.GetString(), buffer.GetSize(), 0);
              }
            } break;
            case CacheMethod::Zrange: {
              auto min_score = doc["min_score"].get_uint64().take_value();
              auto max_score = doc["max_score"].get_uint64().take_value();
              auto result = hcache.zset_zrange(std::move(conn.zkey), min_score, max_score);
              if (!result.has_value()) {
                ff_send(clientfd, HTTP_404, sizeof(HTTP_404) - 1, 0);
              } else {
                auto &&d = rapidjson::Document();
                auto &sv_list = d.SetArray();
                auto &allocator = d.GetAllocator();
                auto &&result_value = result.value();
                for (auto &&svs: result_value) {
                  for (auto &&v: svs.values) {
                    auto &&sv_object = rapidjson::Value(rapidjson::kObjectType);
                    auto &&v_string = rapidjson::StringRef(v.data(), v.size());
                    sv_object.AddMember("score", svs.score, allocator);
                    sv_object.AddMember("value", v_string, allocator);
                    sv_list.PushBack(sv_object, allocator);
                  }
                }
                rapidjson::StringBuffer buffer;
                rapidjson::Writer<rapidjson::StringBuffer> writer(buffer);
                d.Accept(writer);
                char len_str[16] = {0};
                auto len_str_size = snprintf(len_str, sizeof(len_str), CONTENT_LENGTH_FORMAT, buffer.GetSize());
                ff_send(clientfd, HTTP_200, sizeof(HTTP_200) - 1, 0);
                ff_send(clientfd, len_str, len_str_size, 0);
                ff_send(clientfd, buffer.GetString(), buffer.GetSize(), 0);
              }
            }
            default:
              break;
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
  init_storage(hcache);
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