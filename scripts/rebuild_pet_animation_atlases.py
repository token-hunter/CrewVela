from __future__ import annotations

from pathlib import Path

from PIL import Image


ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIR = ROOT / "src" / "assets" / "pets" / "actions"
OUTPUT_DIR = ROOT / "src" / "assets" / "pets" / "animations"

PETS = (
    "apple",
    "banana",
    "grapes",
    "orange-astro",
    "orange-coder",
    "orange-sleepy",
    "orange-spark",
    "strawberry",
)

FRAME_SIZE = 128
SOURCE_COLUMNS = 6
SOURCE_ROWS = 3
OUTPUT_COLUMNS = 10
OUTPUT_ROWS = 6

ACTION_LAYOUT = (
    ("idle", 0, 0),
    ("sleep", 4, 0),
    ("stretch", 7, 0),
    ("walk", 0, 1),
    ("run", 4, 1),
    ("jump", 7, 1),
    ("chase", 0, 2),
    ("read", 3, 2),
    ("coffee", 6, 2),
    ("computer", 0, 3),
    ("sing", 3, 3),
    ("eat", 7, 3),
    ("music", 0, 4),
    ("celebrate", 3, 4),
    ("think", 7, 4),
    ("remind", 0, 5),
    ("tumble", 3, 5),
    ("blink", 7, 5),
)

# The source pack stores the eighteen actions as a regular 6 x 3 grid.
SOURCE_ACTION_INDEX = {name: index for index, (name, _, _) in enumerate(ACTION_LAYOUT)}


def source_cell(atlas: Image.Image, action: str) -> Image.Image:
    # Reminder bubbles and the old tumble cell contain an extra detached drawing.
    # Reuse the clean idle body; the runtime supplies the reminder/tumble motion.
    source_action = "idle" if action in {"blink", "remind", "tumble"} else action
    index = SOURCE_ACTION_INDEX[source_action]
    width = atlas.width // SOURCE_COLUMNS
    height = atlas.height // SOURCE_ROWS
    column = index % SOURCE_COLUMNS
    row = index // SOURCE_COLUMNS
    return atlas.crop((column * width, row * height, (column + 1) * width, (row + 1) * height))


def normalized_subject(cell: Image.Image) -> Image.Image:
    alpha = cell.getchannel("A")
    bounds = alpha.getbbox()
    if bounds is None:
        return Image.new("RGBA", (FRAME_SIZE, FRAME_SIZE))
    subject = cell.crop(bounds)
    subject.thumbnail((FRAME_SIZE - 14, FRAME_SIZE - 14), Image.Resampling.NEAREST)
    frame = Image.new("RGBA", (FRAME_SIZE, FRAME_SIZE))
    frame.alpha_composite(subject, ((FRAME_SIZE - subject.width) // 2, FRAME_SIZE - subject.height - 6))
    return frame


def animation_variant(base: Image.Image, action: str, frame_index: int) -> Image.Image:
    shifts = {
        "walk": ((-3, 1), (0, -3), (3, 1)),
        "run": ((-5, 2), (0, -5), (5, 1)),
        "jump": ((-2, 3), (0, -8), (2, 2)),
        "chase": ((-5, 1), (1, -4), (5, 1)),
        "tumble": ((-2, 1), (0, -3), (2, 1)),
    }
    dx, dy = shifts.get(action, ((0, 1), (0, -2), (0, 1)))[frame_index]
    frame = Image.new("RGBA", base.size)
    frame.alpha_composite(base, (dx, dy))
    safe_bounds = (2, 2, FRAME_SIZE - 2, FRAME_SIZE - 2)
    if frame.getchannel("A").getbbox() and not (
        frame.getchannel("A").getbbox()[0] >= safe_bounds[0]
        and frame.getchannel("A").getbbox()[1] >= safe_bounds[1]
        and frame.getchannel("A").getbbox()[2] <= safe_bounds[2]
        and frame.getchannel("A").getbbox()[3] <= safe_bounds[3]
    ):
        content_bounds = frame.getchannel("A").getbbox()
        assert content_bounds is not None
        content = frame.crop(content_bounds)
        content.thumbnail((FRAME_SIZE - 8, FRAME_SIZE - 8), Image.Resampling.NEAREST)
        frame = Image.new("RGBA", base.size)
        frame.alpha_composite(content, ((FRAME_SIZE - content.width) // 2, FRAME_SIZE - content.height - 4))
    return frame


def count_large_components(frame: Image.Image) -> int:
    alpha = frame.getchannel("A")
    pixels = alpha.load()
    visited: set[tuple[int, int]] = set()
    components = 0
    for y in range(frame.height):
        for x in range(frame.width):
            if pixels[x, y] <= 32 or (x, y) in visited:
                continue
            stack = [(x, y)]
            visited.add((x, y))
            size = 0
            while stack:
                px, py = stack.pop()
                size += 1
                for nx, ny in ((px - 1, py), (px + 1, py), (px, py - 1), (px, py + 1)):
                    if 0 <= nx < frame.width and 0 <= ny < frame.height and (nx, ny) not in visited and pixels[nx, ny] > 32:
                        visited.add((nx, ny))
                        stack.append((nx, ny))
            if size >= 700:
                components += 1
    return components


def rebuild(pet: str) -> None:
    source_path = SOURCE_DIR / f"{pet}-atlas.png"
    output_path = OUTPUT_DIR / f"{pet}-animation.png"
    atlas = Image.open(source_path).convert("RGBA")
    output = Image.new("RGBA", (OUTPUT_COLUMNS * FRAME_SIZE, OUTPUT_ROWS * FRAME_SIZE))
    for action, target_column, target_row in ACTION_LAYOUT:
        base = normalized_subject(source_cell(atlas, action))
        for frame_index in range(3):
            frame = animation_variant(base, action, frame_index)
            component_count = count_large_components(frame)
            if component_count != 1:
                raise RuntimeError(f"{pet}/{action}/{frame_index}: expected one pet, found {component_count}")
            output.alpha_composite(frame, ((target_column + frame_index) * FRAME_SIZE, target_row * FRAME_SIZE))
    output.save(output_path, optimize=True)


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    for pet in PETS:
        rebuild(pet)
        print(f"rebuilt {pet}")


if __name__ == "__main__":
    main()
