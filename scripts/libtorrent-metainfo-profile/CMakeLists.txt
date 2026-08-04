cmake_minimum_required(VERSION 3.20)

project(rstorrent_libtorrent_metainfo_profile LANGUAGES CXX)

find_package(LibtorrentRasterbar CONFIG REQUIRED)

add_executable(libtorrent-metainfo-profile main.cpp)
target_compile_features(libtorrent-metainfo-profile PRIVATE cxx_std_17)
target_link_libraries(
  libtorrent-metainfo-profile
  PRIVATE LibtorrentRasterbar::torrent-rasterbar
)
