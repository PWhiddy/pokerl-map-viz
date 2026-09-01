"""Animated, data-driven AI autonomy plot for Manim Community."""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from manim import (
    Annulus,
    AnimationGroup,
    Arrow,
    Axes,
    Circle,
    Create,
    FadeIn,
    Flash,
    GrowFromCenter,
    LEFT,
    Line,
    ORIGIN,
    RIGHT,
    Scene,
    Square,
    SurroundingRectangle,
    Text,
    UP,
    VGroup,
    Write,
    config,
)
from manim.utils.rate_functions import ease_out_cubic


DATA_PATH = Path(__file__).with_name("autonomy_data.json")


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

        title = Text(
            data["title"],
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

        x_label = Text(
            data["x_axis_label"], font=font, font_size=24, color=muted
        ).next_to(axes, direction=np.array([0.0, -1.0, 0.0]), buff=0.2)
        y_label = VGroup(
            *(
                Text(line, font=font, font_size=23, color=muted)
                for line in data["y_axis_label"].splitlines()
            )
        ).arrange(np.array([0.0, -1.0, 0.0]), buff=0.055)
        y_label.next_to(axes, LEFT, buff=0.3)

        legend = self.make_legends(data, font, text_color, muted)
        legend.to_edge(np.array([0.0, -1.0, 0.0]), buff=0.38)

        intro_time = float(timing["intro_seconds"])
        self.play(Write(title), run_time=intro_time * 0.48)
        self.play(
            AnimationGroup(
                Create(axes),
                FadeIn(x_label, shift=UP * 0.05),
                FadeIn(y_label, shift=RIGHT * 0.05),
                FadeIn(legend, shift=UP * 0.08),
                lag_ratio=0.08,
            ),
            run_time=intro_time * 0.52,
        )

        seconds_per_point = float(timing["seconds_per_point"])
        reveal_time = min(0.66, seconds_per_point * 0.24)
        label_time = min(0.78, seconds_per_point * 0.28)
        hold_time = max(0.0, seconds_per_point - reveal_time - label_time)
        connected_types = set(data.get("connect_types", []))
        previous_positions: dict[str, np.ndarray] = {}

        for point in data["points"]:
            point_type = point["type"]
            color = style["type_colors"].get(point_type, text_color)
            position = axes.c2p(float(point["x"]), float(point["y"]))
            marker = self.make_marker(point, position, color)
            label_group, connector = self.make_label(
                point,
                position,
                marker,
                color,
                font,
                text_color,
                style["background_color"],
            )

            type_arrow = None
            if point_type in connected_types and point_type in previous_positions:
                type_arrow = Arrow(
                    previous_positions[point_type],
                    position,
                    buff=0.135,
                    color=color,
                    stroke_width=2.2,
                    max_tip_length_to_length_ratio=0.16,
                    max_stroke_width_to_length_ratio=4.0,
                )
                type_arrow.set_opacity(float(style["connection_opacity"]))
                type_arrow.get_tip().scale(
                    0.55, about_point=type_arrow.get_end()
                )
                type_arrow.set_z_index(0)
            if point_type in connected_types:
                previous_positions[point_type] = position

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
            if type_arrow is not None:
                reveal_animations.insert(0, Create(type_arrow))
            self.play(*reveal_animations, run_time=reveal_time)
            self.play(
                Create(connector),
                FadeIn(label_group, shift=0.06 * self.offset_unit(point)),
                run_time=label_time,
            )
            if hold_time:
                self.wait(hold_time)

        self.wait(float(timing["final_hold_seconds"]))

    @staticmethod
    def offset_unit(point: dict) -> np.ndarray:
        offset = np.array([*point.get("label_offset", [0.5, 0.5]), 0.0], dtype=float)
        norm = np.linalg.norm(offset)
        return offset / norm if norm else RIGHT

    @staticmethod
    def make_marker(point: dict, position: np.ndarray, color: str) -> VGroup:
        if point.get("marker", "circle").lower() == "x":
            radius = 0.145
            marker = VGroup(
                Line(LEFT * radius + UP * radius, RIGHT * radius - UP * radius),
                Line(LEFT * radius - UP * radius, RIGHT * radius + UP * radius),
            )
            marker.set_stroke(color=color, width=5.0)
            return marker.move_to(position)

        return AIAutonomy.make_circle_marker(
            float(point["fill"]), position, color, radius=0.115, stroke_width=3.1
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
        color: str,
        font: str,
        text_color: str,
        background_color: str,
    ) -> tuple[VGroup, Line]:
        offset = np.array([*point.get("label_offset", [0.5, 0.5]), 0.0], dtype=float)
        label = Text(
            point["label"],
            font=font,
            font_size=19,
            line_spacing=0.82,
            color=text_color,
        ).move_to(position + offset)
        card = SurroundingRectangle(
            label,
            buff=0.085,
            corner_radius=0.07,
            color=color,
            stroke_width=1.05,
            fill_color=background_color,
            fill_opacity=0.93,
        )
        label_group = VGroup(card, label)
        direction = self.offset_unit(point)
        connector = Line(
            position,
            card.get_boundary_point(-direction),
            color=color,
            stroke_width=1.25,
            stroke_opacity=0.72,
        )
        connector.set_z_index(1)
        marker.set_z_index(3)
        label_group.set_z_index(2)
        return label_group, connector

    @staticmethod
    def make_legends(
        data: dict, font: str, text_color: str, muted: str
    ) -> VGroup:
        sample_color = data["style"]["axis_color"]
        feasibility_title = Text(
            data["feasibility_legend_title"], font=font, font_size=18, color=muted
        )
        feasibility_samples = VGroup()
        for fill, label_text in (
            (0.0, "small progress"),
            (1.0, "finish the game"),
        ):
            marker = AIAutonomy.make_circle_marker(
                fill, ORIGIN, sample_color, radius=0.078, stroke_width=2.1
            )
            label = Text(label_text, font=font, font_size=16, color=text_color)
            feasibility_samples.add(VGroup(marker, label).arrange(RIGHT, buff=0.11))
        feasibility_samples.arrange(RIGHT, buff=0.42)
        feasibility_legend = VGroup(feasibility_title, feasibility_samples).arrange(
            RIGHT, buff=0.34
        )

        type_title = Text(data["type_legend_title"], font=font, font_size=18, color=muted)
        type_samples = VGroup()
        for point_type, color in data["style"]["type_colors"].items():
            swatch = Square(side_length=0.12, stroke_width=0, fill_color=color, fill_opacity=1)
            label = Text(point_type, font=font, font_size=16, color=text_color)
            type_samples.add(VGroup(swatch, label).arrange(RIGHT, buff=0.1))
        type_samples.arrange(RIGHT, buff=0.38)
        type_legend = VGroup(type_title, type_samples).arrange(RIGHT, buff=0.34)

        return VGroup(feasibility_legend, type_legend).arrange(
            np.array([0.0, -1.0, 0.0]), buff=0.14
        )
