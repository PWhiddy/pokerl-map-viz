"""Animated, data-driven AI autonomy plot for Manim Community."""

from __future__ import annotations

import json
from html import escape as escape_markup
from pathlib import Path

import numpy as np
from manim import (
    Annulus,
    Axes,
    Circle,
    Create,
    FadeIn,
    FadeOut,
    Flash,
    GrowFromCenter,
    LEFT,
    Line,
    MarkupText,
    ORIGIN,
    Polygon,
    Rectangle,
    RIGHT,
    Scene,
    UP,
    VGroup,
    Write,
    config,
)
from manim.utils.rate_functions import ease_out_cubic, there_and_back


DATA_PATH = Path(__file__).with_name("autonomy_data.json")
POINT_RADIUS = 0.135
X_MARKER_RADIUS = 0.17


def tracked_text(text: str, tracking: int, **kwargs) -> MarkupText:
    """Render a fully shaped Pango text run with consistent global tracking."""
    markup = f'<span letter_spacing="{tracking}">{escape_markup(text)}</span>'
    return MarkupText(markup, **kwargs)


def load_data() -> dict:
    """Load and validate the user-editable scene data."""
    with DATA_PATH.open(encoding="utf-8") as handle:
        data = json.load(handle)

    required = {"label", "x", "y", "fill", "type"}
    for index, point in enumerate(data.get("points", []), start=1):
        missing = required.difference(point)
        if missing:
            raise ValueError(f"Point {index} is missing: {', '.join(sorted(missing))}")
        for field in ("x", "y", "fill"):
            if not 0.0 <= float(point[field]) <= 1.0:
                raise ValueError(f"Point {index} has {field} outside the range 0–1")
    return data


class AIAutonomy(Scene):
    """Build the complete animation from ``autonomy_data.json``."""

    def construct(self) -> None:
        data = load_data()
        style = data["style"]
        timing = data["timing"]
        config.background_color = style["background_color"]

        font = style["font"]
        text_color = style["text_color"]
        muted = style["muted_text_color"]
        axis_color = style["axis_color"]
        axis_label_color = style.get("axis_label_color", muted)
        text_tracking = int(style.get("text_tracking", -128))

        title = tracked_text(
            data["title"],
            tracking=text_tracking,
            font=font,
            font_size=46,
            weight="BOLD",
            color=text_color,
        ).to_edge(UP, buff=0.3)

        axes = Axes(
            x_range=[0, 1, 0.25],
            y_range=[0, 1, 0.25],
            x_length=7.6,
            y_length=4.25,
            tips=False,
            axis_config={
                "color": axis_color,
                "stroke_width": 2.4,
                "include_ticks": False,
            },
        ).shift(UP * 0.18)

        x_label = tracked_text(
            data["x_axis_label"],
            tracking=text_tracking,
            font=font,
            font_size=24,
            color=axis_label_color,
        ).next_to(axes, direction=np.array([0.0, -1.0, 0.0]), buff=0.2)
        y_label = VGroup(
            *(
                tracked_text(
                    line,
                    tracking=text_tracking,
                    font=font,
                    font_size=23,
                    color=axis_label_color,
                )
                for line in data["y_axis_label"].splitlines()
            )
        ).arrange(np.array([0.0, -1.0, 0.0]), buff=0.055)
        y_label.next_to(axes, LEFT, buff=0.3)

        legend_layout, legend_base, type_legend_entries = self.make_legends(
            data, font, text_color, muted
        )
        legend_layout.to_edge(np.array([0.0, -1.0, 0.0]), buff=0.38)

        intro_time = float(timing["intro_seconds"])
        self.play(Write(title), run_time=intro_time * 0.48)

        axis_reveal_time = intro_time * 0.26
        axis_scan_time = float(timing.get("axis_scan_seconds", 1.8))
        scan_color = style.get("axis_scan_color", text_color)
        x_scan = Line(
            UP * 0.17,
            -UP * 0.17,
            color=scan_color,
            stroke_width=5.0,
        ).move_to(axes.c2p(0, 0))
        x_scan.set_z_index(4)
        self.play(
            Create(axes.x_axis),
            FadeIn(x_label, shift=UP * 0.05),
            run_time=axis_reveal_time,
        )
        self.play(GrowFromCenter(x_scan), run_time=0.18)
        self.play(
            x_scan.animate.move_to(axes.c2p(1, 0)),
            run_time=axis_scan_time,
            rate_func=there_and_back,
        )
        self.play(FadeOut(x_scan), run_time=0.18)

        y_scan = Line(
            LEFT * 0.17,
            RIGHT * 0.17,
            color=scan_color,
            stroke_width=5.0,
        ).move_to(axes.c2p(0, 0))
        y_scan.set_z_index(4)
        self.play(
            Create(axes.y_axis),
            FadeIn(y_label, shift=RIGHT * 0.05),
            run_time=axis_reveal_time,
        )
        self.play(GrowFromCenter(y_scan), run_time=0.18)
        self.play(
            y_scan.animate.move_to(axes.c2p(0, 1)),
            run_time=axis_scan_time,
            rate_func=there_and_back,
        )
        self.play(FadeOut(y_scan), run_time=0.18)

        orientation_guides = self.make_orientation_guides(data, axes, font)
        orientation_time = float(timing.get("orientation_seconds", 3.2))
        transition_time = min(0.5, orientation_time * 0.16)
        stagger_hold = min(0.6, orientation_time * 0.19)
        lower_guide, upper_guide = orientation_guides
        self.play(
            FadeIn(lower_guide, shift=UP * 0.04),
            run_time=transition_time,
        )
        self.wait(stagger_hold)
        self.play(
            FadeIn(upper_guide, shift=UP * 0.04),
            run_time=transition_time,
        )
        self.wait(
            max(0.0, orientation_time - 3 * transition_time - stagger_hold)
        )
        self.play(FadeOut(orientation_guides), run_time=transition_time)

        seconds_per_point = float(timing["seconds_per_point"])
        reveal_time = min(0.66, seconds_per_point * 0.24)
        label_time = min(0.78, seconds_per_point * 0.28)
        hold_time = max(0.0, seconds_per_point - reveal_time - label_time)
        connected_types = set(data.get("connect_types", []))
        previous_positions: dict[str, tuple[np.ndarray, dict]] = {}
        revealed_types: set[str] = set()
        progress_legend_revealed = False

        for point in data["points"]:
            point_type = point["type"]
            color = style["type_colors"].get(point_type, text_color)
            position = axes.c2p(float(point["x"]), float(point["y"]))
            marker = self.make_marker(point, position, color)
            label_group = self.make_label(
                point,
                position,
                marker,
                font,
                text_color,
                float(style.get("point_label_font_size", 17)),
                text_tracking,
            )

            type_arrow = None
            if point_type in connected_types and point_type in previous_positions:
                type_arrow = self.make_fixed_arrow(
                    previous_positions[point_type][0],
                    position,
                    color,
                    float(style["connection_opacity"]),
                    float(style.get("connection_stroke_width", 2.6)),
                    float(style.get("arrow_head_width", 0.126)),
                    previous_positions[point_type][1],
                    point,
                )
                type_arrow.set_z_index(0)
            is_transient = bool(point.get("transient", False))
            if point_type in connected_types and not is_transient:
                previous_positions[point_type] = (position, point)

            reveal_animations = [
                GrowFromCenter(marker, rate_func=ease_out_cubic),
                Flash(
                    position,
                    color=color,
                    flash_radius=0.28,
                    line_length=0.09,
                    num_lines=12,
                ),
            ]
            if point_type not in revealed_types:
                reveal_animations.append(
                    FadeIn(type_legend_entries[point_type], shift=RIGHT * 0.05)
                )
                revealed_types.add(point_type)
            if type_arrow is not None:
                reveal_animations.insert(0, Create(type_arrow))
            self.play(*reveal_animations, run_time=reveal_time)
            self.play(
                FadeIn(label_group, shift=0.06 * self.offset_unit(point)),
                run_time=label_time,
            )
            if (
                point.get("introduces_progress_legend", False)
                and not progress_legend_revealed
            ):
                self.wait(float(timing.get("progress_legend_delay_seconds", 1.0)))
                self.play(
                    FadeIn(legend_base, shift=UP * 0.08),
                    marker.animate(rate_func=there_and_back).scale(1.2),
                    run_time=float(timing.get("legend_reveal_seconds", 0.55)),
                )
                progress_legend_revealed = True
            if hold_time:
                self.wait(hold_time)
            if is_transient:
                transient_objects = [marker, label_group]
                if type_arrow is not None:
                    transient_objects.append(type_arrow)
                self.play(
                    *(FadeOut(mobject) for mobject in transient_objects),
                    run_time=float(timing.get("transient_fade_seconds", 0.45)),
                )

        self.wait(float(timing["final_hold_seconds"]))

    @staticmethod
    def make_orientation_guides(data: dict, axes: Axes, font: str) -> VGroup:
        """Create temporary shaded explanations for the two autonomy extremes."""
        guide_data = data["orientation_guides"]
        style = data["style"]
        region_opacity = float(style.get("orientation_region_opacity", 0.13))
        label_size = float(style.get("orientation_label_font_size", 17))

        def guide(
            lower_left: tuple[float, float],
            upper_right: tuple[float, float],
            label_position: tuple[float, float],
            label: str,
            color: str,
        ) -> VGroup:
            lower = axes.c2p(*lower_left)
            upper = axes.c2p(*upper_right)
            region = Rectangle(
                width=upper[0] - lower[0],
                height=upper[1] - lower[1],
                stroke_width=1.4,
                stroke_color=color,
                stroke_opacity=0.42,
                fill_color=color,
                fill_opacity=region_opacity,
            ).move_to((lower + upper) / 2)
            region.set_z_index(-1)
            text = tracked_text(
                label,
                tracking=int(style.get("text_tracking", -128)),
                font=font,
                font_size=label_size,
                line_spacing=0.78,
                color=color,
            ).move_to(axes.c2p(*label_position))
            text.set_z_index(1)
            return VGroup(region, text)

        lower_guide = guide(
            (0.01, 0.01),
            (0.48, 0.48),
            (0.245, 0.245),
            guide_data["lower_left_label"],
            guide_data["lower_left_color"],
        )
        upper_guide = guide(
            (0.52, 0.52),
            (0.99, 0.99),
            (0.755, 0.755),
            guide_data["upper_right_label"],
            guide_data["upper_right_color"],
        )
        return VGroup(lower_guide, upper_guide)

    @staticmethod
    def offset_unit(point: dict) -> np.ndarray:
        offset = np.array([*point.get("label_offset", [0.5, 0.5]), 0.0], dtype=float)
        norm = np.linalg.norm(offset)
        return offset / norm if norm else RIGHT

    @staticmethod
    def make_marker(point: dict, position: np.ndarray, color: str) -> VGroup:
        if point.get("marker", "circle").lower() == "x":
            radius = X_MARKER_RADIUS
            marker = VGroup(
                Line(LEFT * radius + UP * radius, RIGHT * radius - UP * radius),
                Line(LEFT * radius - UP * radius, RIGHT * radius + UP * radius),
            )
            marker.set_stroke(color=color, width=5.0)
            return marker.move_to(position)

        return AIAutonomy.make_circle_marker(
            float(point["fill"]), position, color, radius=POINT_RADIUS, stroke_width=3.2
        )

    @staticmethod
    def make_circle_marker(
        fill: float,
        position: np.ndarray,
        color: str,
        radius: float,
        stroke_width: float,
    ) -> VGroup:
        """Fill exactly ``fill`` of the disk's area, moving outside-in."""
        outline = (
            Circle(radius=radius)
            .set_stroke(color=color, width=stroke_width, opacity=1.0)
            .set_fill(opacity=0.0)
            .move_to(position)
        )
        pieces = VGroup()
        if fill >= 1.0:
            pieces.add(
                Circle(radius=radius * 0.91)
                .set_stroke(width=0)
                .set_fill(color=color, opacity=1.0)
                .move_to(position)
            )
        elif fill > 0.0:
            fill_radius = radius * 0.91
            ring = Annulus(
                inner_radius=fill_radius * np.sqrt(1.0 - fill),
                outer_radius=fill_radius,
                color=color,
                fill_opacity=1.0,
                stroke_width=0,
            )
            pieces.add(ring.move_to(position))
        pieces.add(outline)
        return pieces

    def make_label(
        self,
        point: dict,
        position: np.ndarray,
        marker: VGroup,
        font: str,
        text_color: str,
        default_font_size: float,
        text_tracking: int,
    ) -> VGroup:
        offset = np.array([*point.get("label_offset", [0.5, 0.5]), 0.0], dtype=float)
        label = tracked_text(
            point["label"],
            tracking=text_tracking,
            font=font,
            font_size=float(point.get("label_font_size", default_font_size)),
            line_spacing=float(point.get("label_line_spacing", 0.76)),
            color=text_color,
        ).move_to(position + offset)
        label_group = VGroup(label)
        marker.set_z_index(3)
        label_group.set_z_index(2)
        return label_group

    @staticmethod
    def make_fixed_arrow(
        start: np.ndarray,
        end: np.ndarray,
        color: str,
        opacity: float,
        stroke_width: float,
        head_width: float,
        start_point: dict,
        end_point: dict,
    ) -> VGroup:
        """Build an arrow with a fixed head, joined flush to its shaft."""
        displacement = end - start
        distance = np.linalg.norm(displacement)
        if distance == 0:
            return VGroup()

        direction = displacement / distance
        perpendicular = np.array([-direction[1], direction[0], 0.0])
        start_clearance = AIAutonomy.marker_clearance(start_point, direction)
        end_clearance = AIAutonomy.marker_clearance(end_point, direction)
        head_length = 0.11
        head_half_width = head_width / 2

        shaft_start = start + direction * start_clearance
        tip = end - direction * end_clearance
        head_base = tip - direction * head_length
        shaft = Line(
            shaft_start,
            head_base,
            color=color,
            stroke_width=stroke_width,
            stroke_opacity=opacity,
        )
        head = Polygon(
            tip,
            head_base + perpendicular * head_half_width,
            head_base - perpendicular * head_half_width,
            stroke_width=0,
            fill_color=color,
            fill_opacity=opacity,
        )
        return VGroup(shaft, head)

    @staticmethod
    def marker_clearance(point: dict, direction: np.ndarray) -> float:
        """Distance from marker center to its outer edge along an arrow."""
        if point.get("marker", "circle").lower() == "x":
            cross_radius = X_MARKER_RADIUS
            return cross_radius * (abs(direction[0]) + abs(direction[1]))
        return POINT_RADIUS

    @staticmethod
    def make_legends(
        data: dict, font: str, text_color: str, muted: str
    ) -> tuple[VGroup, VGroup, dict[str, VGroup]]:
        legend_font = data["style"].get("legend_font", font)
        sample_color = data["style"]["axis_color"]
        text_tracking = int(data["style"].get("text_tracking", -128))
        feasibility_title = tracked_text(
            data["feasibility_legend_title"],
            tracking=text_tracking,
            font=legend_font,
            font_size=18,
            color=muted,
            disable_ligatures=False,
        )
        feasibility_samples = VGroup()
        for fill, label_text in (
            (0.0, "None"),
            (1.0, "Finishes the game"),
        ):
            marker = AIAutonomy.make_circle_marker(
                fill, ORIGIN, sample_color, radius=0.078, stroke_width=2.1
            )
            label = tracked_text(
                label_text,
                tracking=text_tracking,
                font=legend_font,
                font_size=16,
                color=text_color,
                disable_ligatures=False,
            )
            feasibility_samples.add(VGroup(marker, label).arrange(RIGHT, buff=0.08))
        feasibility_samples.arrange(RIGHT, buff=0.28)
        feasibility_legend = VGroup(feasibility_title, feasibility_samples).arrange(
            RIGHT, buff=0.24
        )

        type_samples = VGroup()
        type_entries: dict[str, VGroup] = {}
        display_labels = data["style"].get("type_labels", {})
        for point_type, color in data["style"]["type_colors"].items():
            swatch = Circle(radius=0.06, stroke_width=0, fill_color=color, fill_opacity=1)
            label = tracked_text(
                display_labels.get(point_type, point_type),
                tracking=text_tracking,
                font=legend_font,
                font_size=16,
                color=text_color,
                disable_ligatures=False,
            )
            entry = VGroup(swatch, label).arrange(RIGHT, buff=0.08)
            type_entries[point_type] = entry
            type_samples.add(entry)
        type_samples.arrange(RIGHT, buff=0.24)

        legend_layout = VGroup(feasibility_legend, type_samples).arrange(
            np.array([0.0, -1.0, 0.0]), buff=0.14
        )
        legend_base = VGroup(feasibility_legend)
        return legend_layout, legend_base, type_entries
