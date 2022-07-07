#include <arpa/inet.h>
#include <assert.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <sys/types.h>

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

extern char html1[1], html2[1], html[1];
extern int ff_zc_mbuf_get(struct ff_zc_mbuf *m, int len);
extern int ff_zc_mbuf_write(struct ff_zc_mbuf *m, const char *data, int len);

char *buf_tmp;
char html_buf[10240];
size_t buf_len = 0;
struct ff_zc_mbuf zc_buf;

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

        if (ff_kevent(kq, &kevSet, 1, NULL, 0, NULL) < 0) {
          printf("ff_kevent error:%d, %s\n", errno, strerror(errno));
          return -1;
        }

        available--;
      } while (available);
    } else if (event.filter == EVFILT_READ) {
      char buf[256];
      size_t readlen = ff_read(clientfd, buf, sizeof(buf));
      int ret = ff_zc_mbuf_get(&zc_buf, buf_len);
      if (ret < 0) {
        printf("ff_zc_mbuf_get failed, len:%d, errno:%d, %s\n", buf_len, errno, strerror(errno));
        exit(1);
      }

      /* APP can call ff_zc_mbuf_write multi times */
      int len_part = 1440, off, to_write_len;
      for (off = 0; off < buf_len;) {
        to_write_len = (buf_len - off) > len_part ? len_part : (buf_len - off);
        ret = ff_zc_mbuf_write(&zc_buf, (const char *) buf_tmp + off, to_write_len);
        if (ret != to_write_len) {
          printf("ff_zc_mbuf_write failed, len:%d, errno:%d, %s\n", to_write_len, errno, strerror(errno));
          exit(1);
        }
        off += to_write_len;
      }

      /* Or call ff_zc_mbuf_write one time */
      /*
            if (ret != buf_len) {
                printf("ff_zc_mbuf_write failed, len:%d, errno:%d, %s\n", buf_len, errno, strerror(errno));
                exit(1);
            }
            */

      /* Simulate the application load */
      int i, j = 0;
      for (i = 0; i < 10000; i++) { j++; }
      ff_write(clientfd, zc_buf.bsd_mbuf, buf_len);

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