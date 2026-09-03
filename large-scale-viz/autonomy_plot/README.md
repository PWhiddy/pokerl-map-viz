# AI Autonomy animation

The animation is implemented with Manim Community and reads all plot content from
`autonomy_data.json`. Edit that JSON file to change the title, axes, timing, type
colors, point coordinates, feasibility, marker type, or label placement without
modifying the Python scene.

The temporary `orientation_guides` introduce the upper-right and lower-left
extremes before any points appear. Their duration and visual styling are
controlled by `timing.orientation_seconds` and the `orientation_*` style keys.
The x- and y-axis sweeps use `timing.axis_scan_seconds` and
`style.axis_scan_color`. The x-axis is introduced and scanned first, followed by
the y-axis; the lower-left orientation guide then precedes the upper-right one.
`style.axis_label_color` independently controls the axis-label contrast.

`style.text_mobject_scale_factor` applies the Manim text-rendering workaround used
here for improved kerning on Ubuntu. Its value is `0.0016666666666666668`
instead of Manim's native `0.05`; the scene automatically requests fonts at 30x
size before Manim scales them down, so nominal displayed font sizes stay
unchanged. The helper also expands Pango's temporary layout canvas to prevent
long text from wrapping at this scale. This is local to this scene and does not
modify the `science` Conda environment.

Coordinates and `fill` must be in the range 0–1. Labels default to `name`; add an
`label` can contain `\n` when you want a controlled line break. Point type is
encoded only by color; edit `style.type_colors` to change the mapping. `Script`
groups the Random and TAS points. Each type's legend entry appears with its first
point while retaining a fixed legend layout; `style.type_labels` controls the
long-form names shown there. A point with `"introduces_progress_legend": true`
reveals the progress legend after its own entrance and briefly pulses to connect
the legend explanation to the marker. Use
`"marker": "x"` for a crossed marker, and tune `label_offset: [x, y]` in Manim
screen units if a label needs to move. `label_font_size` and
`label_line_spacing` optionally override the global `style.point_label_font_size`.
Set `"transient": true` to show a point briefly without making it the origin of
the next connection; its marker, label, and incoming arrow fade out together.
A circle fills from its outer edge inward;
the colored ring covers exactly the area proportion specified by `fill` (for
example, `0.15` colors 15% of the disk area). Types listed in `connect_types`
receive animated arrows in data order. Their body and head weights are controlled
by `style.connection_stroke_width` and `style.arrow_head_width`.

Render the final 4K animation in the requested environment:

```bash
./render_4k.sh
```

The wrapper uses the `science` Conda environment, disables Manim's stale-frame
cache, and decodes each rendered segment independently before concatenation to
avoid intermittent missing-glyph artifacts at H.264 segment boundaries. The
delivery encode uses one-second keyframes and no B-frames so scrubbing and exact
frame extraction also preserve every glyph.

For a quick direct preview, run
`conda run -n science manim --disable_caching -ql autonomy_plot.py AIAutonomy`.
The 4K wrapper writes the final video to
`media/videos/autonomy_plot/2160p60/AIAutonomy.mp4`.
