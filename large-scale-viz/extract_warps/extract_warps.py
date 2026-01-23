#!/usr/bin/env python3
"""
Extract all warps and map connections from Pokemon Red and output them as JSON.
Format: { "[x,y,map_id]-[x,y,map_id]": "warp" or "overworld", ... }
"""

import re
import json
from pathlib import Path
from collections import defaultdict

# Build map name to ID mapping from constants file
def parse_map_constants(constants_file):
    """Parse the map_constants.asm file to get map name -> ID, width, height mapping"""
    map_name_to_id = {}
    map_dimensions = {}
    current_id = 0

    with open(constants_file, 'r') as f:
        for line in f:
            # Look for map_const lines: map_const NAME, WIDTH, HEIGHT
            match = re.match(r'\s*map_const\s+(\w+),\s+(\d+),\s+(\d+)', line)
            if match:
                map_name = match.group(1)
                width = int(match.group(2))
                height = int(match.group(3))
                map_name_to_id[map_name] = current_id
                map_dimensions[map_name] = (width, height)
                current_id += 1
            # Handle const_def to reset counter
            elif 'const_def' in line:
                current_id = 0

    # Add special LAST_MAP constant
    map_name_to_id['LAST_MAP'] = 0xFF

    return map_name_to_id, map_dimensions

# Parse a single map object file to extract warps
def parse_map_file(map_file):
    """Parse a map .asm file and extract warp events"""
    warps = []
    in_warp_section = False

    with open(map_file, 'r') as f:
        for line in f:
            # Check if we're entering warp section
            if 'def_warp_events' in line:
                in_warp_section = True
                continue

            # Check if we're leaving warp section
            if in_warp_section and ('def_bg_events' in line or 'def_object_events' in line):
                break

            # Parse warp_event lines
            if in_warp_section:
                match = re.match(r'\s*warp_event\s+(\d+),\s+(\d+),\s+(\w+),\s+(\d+)', line)
                if match:
                    x = int(match.group(1))
                    y = int(match.group(2))
                    dest_map = match.group(3)
                    dest_warp_id = int(match.group(4))
                    warps.append({
                        'x': x,
                        'y': y,
                        'dest_map': dest_map,
                        'dest_warp_id': dest_warp_id
                    })

    return warps

def parse_map_header(header_file):
    """Parse a map header file and extract connections"""
    connections = []

    with open(header_file, 'r') as f:
        for line in f:
            # Parse connection lines: connection direction, MapName, MAP_ID, offset
            match = re.match(r'\s*connection\s+(north|south|east|west),\s+(\w+),\s+(\w+),\s+(-?\d+)', line)
            if match:
                direction = match.group(1)
                dest_map_label = match.group(2)  # CamelCase name
                dest_map_const = match.group(3)  # UPPER_SNAKE_CASE name
                offset = int(match.group(4))
                connections.append({
                    'direction': direction,
                    'dest_map': dest_map_const,
                    'offset': offset
                })

    return connections

def main():
    # Paths
    pokered_dir = Path('pokered')
    constants_file = pokered_dir / 'constants' / 'map_constants.asm'
    maps_objects_dir = pokered_dir / 'data' / 'maps' / 'objects'
    maps_headers_dir = pokered_dir / 'data' / 'maps' / 'headers'

    # Parse map constants
    print("Parsing map constants...")
    map_name_to_id, map_dimensions = parse_map_constants(constants_file)
    print(f"Found {len(map_name_to_id)} maps")

    # Build reverse mapping for getting map name from filename
    # Map files are named like "PalletTown.asm" and the constant is "PALLET_TOWN"
    def filename_to_constant(filename):
        # Remove .asm extension
        name = filename.replace('.asm', '')
        # Convert from CamelCase to UPPER_SNAKE_CASE
        # Insert underscore before capital letters
        result = re.sub('([a-z])([A-Z0-9])', r'\1_\2', name)
        result = re.sub('([A-Z]+)([A-Z][a-z])', r'\1_\2', result)
        # Handle numbers: add underscore before numbers if preceded by letter
        result = re.sub('([a-z])([0-9])', r'\1_\2', result)
        # Add underscore after numbers if followed by uppercase letter (but not for floor numbers like 1F, 2F, B1F)
        # Only add underscore if the letter after the number is NOT 'F', or if F is followed by more letters
        result = re.sub('([0-9])([A-CEG-Z])', r'\1_\2', result)  # Any letter except F
        result = re.sub('([0-9]F)([A-Z])', r'\1_\2', result)  # Floor number followed by more letters
        # Special case: handle "16Fly" -> "16_Fly"
        result = re.sub('([0-9])(Fly)', r'\1_\2', result)
        result = result.upper()
        return result

    # Parse all map files and collect warps
    print("\nParsing map object files for warps...")
    all_map_warps = {}  # map_id -> list of warps

    for map_file in sorted(maps_objects_dir.glob('*.asm')):
        filename = map_file.name
        map_constant = filename_to_constant(filename)

        if map_constant not in map_name_to_id:
            print(f"Warning: No map ID found for {map_constant} ({filename})")
            continue

        map_id = map_name_to_id[map_constant]
        warps = parse_map_file(map_file)

        if warps:
            all_map_warps[map_id] = warps
            print(f"  {map_constant} (ID {map_id:02X}): {len(warps)} warps")

    # Parse all map header files and collect connections
    print("\nParsing map header files for connections...")
    all_map_connections = {}  # map_id -> list of connections

    for header_file in sorted(maps_headers_dir.glob('*.asm')):
        filename = header_file.name
        map_constant = filename_to_constant(filename)

        if map_constant not in map_name_to_id:
            print(f"Warning: No map ID found for {map_constant} ({filename})")
            continue

        map_id = map_name_to_id[map_constant]
        connections = parse_map_header(header_file)

        if connections:
            all_map_connections[map_id] = connections
            print(f"  {map_constant} (ID {map_id:02X}): {len(connections)} connections")

    # Now build the transitions dictionary
    print("\nBuilding transition dictionary...")
    transitions = {}

    # Process warps
    print("  Processing warps...")
    for source_map_id, warps in all_map_warps.items():
        for warp in warps:
            source_x = warp['x']
            source_y = warp['y']
            dest_map_name = warp['dest_map']
            dest_warp_id = warp['dest_warp_id']

            # Get destination map ID
            if dest_map_name not in map_name_to_id:
                print(f"Warning: Unknown destination map {dest_map_name}")
                continue

            dest_map_id = map_name_to_id[dest_map_name]

            # Special case: LAST_MAP warps don't have fixed destination
            if dest_map_id == 0xFF:
                continue

            # Get destination warp coordinates
            if dest_map_id not in all_map_warps:
                print(f"Warning: No warp data for destination map {dest_map_name} (ID {dest_map_id:02X})")
                continue

            dest_warps = all_map_warps[dest_map_id]

            # Warp IDs are 1-indexed, so we need to subtract 1
            dest_warp_idx = dest_warp_id - 1

            if dest_warp_idx < 0 or dest_warp_idx >= len(dest_warps):
                print(f"Warning: Invalid warp ID {dest_warp_id} for map {dest_map_name} (has {len(dest_warps)} warps)")
                continue

            dest_warp = dest_warps[dest_warp_idx]
            dest_x = dest_warp['x']
            dest_y = dest_warp['y']

            # Create bidirectional warp entries
            key1 = f"[{source_map_id}]-[{dest_map_id}]"
            key2 = f"[{dest_map_id}]-[{source_map_id}]"
            transitions[key1] = "warp"
            transitions[key2] = "warp"

    print(f"    Added {len([v for v in transitions.values() if v == 'warp'])} warp transitions")

    # Process map connections
    print("  Processing overworld connections...")
    connection_count = 0

    for source_map_id, connections in all_map_connections.items():
        # Get source map name for looking up dimensions
        source_map_name = None
        for name, map_id in map_name_to_id.items():
            if map_id == source_map_id and name != 'LAST_MAP':
                source_map_name = name
                break

        if not source_map_name or source_map_name not in map_dimensions:
            print(f"Warning: No dimensions found for map ID {source_map_id:02X}")
            continue

        src_width, src_height = map_dimensions[source_map_name]

        for conn in connections:
            direction = conn['direction']
            dest_map_name = conn['dest_map']
            offset = conn['offset']

            if dest_map_name not in map_name_to_id:
                print(f"Warning: Unknown destination map {dest_map_name}")
                continue

            dest_map_id = map_name_to_id[dest_map_name]

            if dest_map_name not in map_dimensions:
                print(f"Warning: No dimensions found for {dest_map_name}")
                continue

            dest_width, dest_height = map_dimensions[dest_map_name]

            # Generate coordinate pairs based on direction
            if direction == 'north':
                # Walking north from current map (y=0) to dest map (y=dest_height-1)
                # x coordinates span the width, adjusted by offset
                for x in range(src_width):
                    dest_x = x + offset
                    if 0 <= dest_x < dest_width:
                        key1 = f"[{source_map_id}]-[{dest_map_id}]"
                        key2 = f"[{dest_map_id}]-[{source_map_id}]"
                        transitions[key1] = "overworld"
                        transitions[key2] = "overworld"
                        connection_count += 2

            elif direction == 'south':
                # Walking south from current map (y=src_height-1) to dest map (y=0)
                for x in range(src_width):
                    dest_x = x + offset
                    if 0 <= dest_x < dest_width:
                        key1 = f"[{source_map_id}]-[{dest_map_id}]"
                        key2 = f"[{dest_map_id}]-[{source_map_id}]"
                        transitions[key1] = "overworld"
                        transitions[key2] = "overworld"
                        connection_count += 2

            elif direction == 'west':
                # Walking west from current map (x=0) to dest map (x=dest_width-1)
                for y in range(src_height):
                    dest_y = y + offset
                    if 0 <= dest_y < dest_height:
                        key1 = f"[{source_map_id}]-[{dest_map_id}]"
                        key2 = f"[{dest_map_id}]-[{source_map_id}]"
                        transitions[key1] = "overworld"
                        transitions[key2] = "overworld"
                        connection_count += 2

            elif direction == 'east':
                # Walking east from current map (x=src_width-1) to dest map (x=0)
                for y in range(src_height):
                    dest_y = y + offset
                    if 0 <= dest_y < dest_height:
                        key1 = f"[{source_map_id}]-[{dest_map_id}]"
                        key2 = f"[{dest_map_id}]-[{source_map_id}]"
                        transitions[key1] = "overworld"
                        transitions[key2] = "overworld"
                        connection_count += 2

    print(f"    Added {connection_count} overworld connection transitions")
    print(f"\nTotal transitions: {len(transitions)}")

    # Save to JSON
    output_file = 'transitions_weak.json'
    with open(output_file, 'w') as f:
        json.dump(transitions, f, indent=2, sort_keys=True)

    print(f"\nTransitions saved to {output_file}")

    # Also save map name to ID mapping for reference
    map_info_file = 'map_info.json'
    map_info = {
        'map_ids': map_name_to_id,
        'map_dimensions': {name: {'width': dims[0], 'height': dims[1]}
                          for name, dims in map_dimensions.items()}
    }
    with open(map_info_file, 'w') as f:
        json.dump(map_info, f, indent=2, sort_keys=True)

    print(f"Map info saved to {map_info_file}")

if __name__ == '__main__':
    main()
