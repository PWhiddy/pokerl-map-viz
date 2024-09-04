#!/bin/bash

# Define an array with the different versions
versions=("extra_fast" "fast" "medium" "full")

# Loop through each version and run the ffmpeg command
for version in "${versions[@]}"; do
    echo "Processing version: $version"
    ffmpeg -framerate 60 -i images/coord_map_${version}_%d.exr \
    -vf "crop=512:512:0:0,curves=all='0/0 0.0002/0.01 1/1',scale=iw*8:ih*8:flags=neighbor" \
    -c:v prores_ks -profile:v 3 "output_${version}.mov"
done

echo "Processing complete!"
