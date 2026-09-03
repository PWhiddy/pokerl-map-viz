#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
render_root=$(mktemp -d)

cleanup() {
    if [[ -n "${render_root:-}" && -d "$render_root" ]]; then
        rm -rf -- "$render_root"
    fi
}
trap cleanup EXIT

conda run --no-capture-output -n science \
    manim --disable_caching --media_dir "$render_root" -qk \
    "$script_dir/autonomy_plot.py" AIAutonomy

partial_dir="$render_root/videos/autonomy_plot/2160p60/partial_movie_files/AIAutonomy"
fixed_movie="$render_root/AIAutonomy.mp4"
final_dir="$script_dir/media/videos/autonomy_plot/2160p60"
final_movie="$final_dir/AIAutonomy.mp4"

mapfile -t clips < <(
    find "$partial_dir" -maxdepth 1 -type f -name 'uncached_*.mp4' | sort
)

if (( ${#clips[@]} == 0 )); then
    echo "No Manim partial clips were produced." >&2
    exit 1
fi

ffmpeg_inputs=()
concat_inputs=""
for index in "${!clips[@]}"; do
    ffmpeg_inputs+=(-threads 1 -i "${clips[$index]}")
    concat_inputs+="[$index:v:0]"
done

# Decode each clip independently before concatenation. Reusing a decoder across
# H.264 segment boundaries can otherwise drop static glyphs on this platform.
ffmpeg -hide_banner -loglevel warning -y \
    "${ffmpeg_inputs[@]}" \
    -filter_complex "${concat_inputs}concat=n=${#clips[@]}:v=1:a=0[video]" \
    -map "[video]" -an \
    -c:v libx264 -preset fast -crf 15 -pix_fmt yuv420p \
    -g 60 -keyint_min 60 -sc_threshold 0 -bf 0 -movflags +faststart \
    "$fixed_movie"

mkdir -p -- "$final_dir"
mv -f -- "$fixed_movie" "$final_movie"
echo "Rendered: $final_movie"
