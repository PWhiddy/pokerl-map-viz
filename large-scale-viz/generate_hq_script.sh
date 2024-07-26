ffmpeg -framerate 60 -i images/coord_map_fast_%d.exr -vf "crop=512:512:0:0,curves=all='0/0 0.0002/0.01 1/1',scale=iw*8:ih*8:flags=neighbor" -c:v prores_ks -profile:v 3 fast_hq.mov

# ^ this does "fast" images, do same for "medium" and "full"
