#include <libtorrent/bdecode.hpp>
#include <libtorrent/error_code.hpp>
#include <libtorrent/info_hash.hpp>
#include <libtorrent/sha1_hash.hpp>
#include <libtorrent/span.hpp>
#include <libtorrent/torrent_info.hpp>

#include <sys/resource.h>

#include <chrono>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

namespace lt = libtorrent;

namespace {

std::vector<char> read_file(std::string const& path)
{
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    if (!input) throw std::runtime_error("failed to open input");
    auto const end = input.tellg();
    if (end < 0) throw std::runtime_error("failed to determine input length");
    std::vector<char> bytes(static_cast<std::size_t>(end));
    input.seekg(0);
    if (!bytes.empty()) input.read(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    if (!input) throw std::runtime_error("failed to read input");
    return bytes;
}

std::uint64_t peak_rss_bytes()
{
    rusage usage{};
    if (getrusage(RUSAGE_SELF, &usage) != 0) return 0;
#if defined(__APPLE__)
    return static_cast<std::uint64_t>(usage.ru_maxrss);
#else
    return static_cast<std::uint64_t>(usage.ru_maxrss) * 1024;
#endif
}

void print_result(
    char const* profile,
    std::size_t input_bytes,
    std::chrono::steady_clock::duration elapsed,
    std::uint64_t baseline_rss,
    lt::torrent_info const& torrent)
{
    std::uint64_t path_bytes = 0;
    for (lt::file_index_t index{0}; index < torrent.files().end_file(); ++index)
        path_bytes += torrent.files().file_path(index).size();

    auto const wall_us =
        std::chrono::duration_cast<std::chrono::microseconds>(elapsed).count();
    auto const peak = peak_rss_bytes();
    std::cout
        << "implementation=libtorrent"
        << " profile=" << profile
        << " accepted=true"
        << " input_bytes=" << input_bytes
        << " info_bytes=" << torrent.info_section().size()
        << " files=" << torrent.num_files()
        << " pieces=" << torrent.num_pieces()
        << " path_bytes=" << path_bytes
        << " trackers=" << torrent.trackers().size()
        << " wall_us=" << wall_us
        << " baseline_peak_rss_bytes=" << baseline_rss
        << " peak_rss_bytes=" << peak
        << " incremental_peak_rss_bytes=" << (peak > baseline_rss ? peak - baseline_rss : 0)
        << '\n';
}

lt::torrent_info parse_explicit(lt::span<char const> bytes)
{
    lt::load_torrent_limits limits;
    limits.max_buffer_size = std::numeric_limits<int>::max();
    limits.max_pieces = 0x200000;
    limits.max_decode_depth = 100;
    limits.max_decode_tokens = 3000000;
    return lt::torrent_info(bytes, limits, lt::from_span);
}

lt::torrent_info parse_peer(lt::span<char const> bytes)
{
    lt::error_code error;
    auto const root = lt::bdecode(bytes, error, nullptr, 200, 2500000);
    if (error) throw lt::system_error(error);

    lt::torrent_info torrent{lt::info_hash_t{lt::sha1_hash{}}};
    if (!torrent.parse_info_section(root, error, 0x200000))
        throw lt::system_error(error);
    return torrent;
}

} // namespace

int main(int argc, char** argv)
{
    if (argc != 3 || (std::string(argv[1]) != "explicit" && std::string(argv[1]) != "peer"))
    {
        std::cerr << "usage: libtorrent-metainfo-profile explicit|peer INPUT\n";
        return 2;
    }

    try
    {
        auto const bytes = read_file(argv[2]);
        auto const baseline_rss = peak_rss_bytes();
        auto const started = std::chrono::steady_clock::now();
        auto const torrent = std::string(argv[1]) == "explicit"
            ? parse_explicit(bytes)
            : parse_peer(bytes);
        print_result(
            argv[1], bytes.size(), std::chrono::steady_clock::now() - started,
            baseline_rss, torrent);
        return 0;
    }
    catch (std::exception const& error)
    {
        std::cout
            << "implementation=libtorrent"
            << " profile=" << argv[1]
            << " accepted=false"
            << " error=" << error.what()
            << '\n';
        return 1;
    }
}
