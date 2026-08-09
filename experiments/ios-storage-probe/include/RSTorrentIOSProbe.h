#ifndef RSTorrentIOSProbe_h
#define RSTorrentIOSProbe_h

#include <stdint.h>

char *rstorrent_ios_probe_run_storage(const char *root);
char *rstorrent_ios_probe_run_network(
    const char *host,
    uint16_t tcp_port,
    uint16_t udp_port
);
void rstorrent_ios_probe_free_json(char *value);

#endif
