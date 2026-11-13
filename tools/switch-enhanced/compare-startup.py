"""Report directly logged startup milestones for baseline or Enhanced runs."""
import argparse
from pathlib import Path
import re

LEGACY = {
    "loading_frame": r"loading screen shown after (\d+) ms",
    "ui_bundle": r"UI bundle ready after (\d+) ms",
    "core": r"core-ready after (\d+) ms",
    "world_renderer": r"world renderer ready after (\d+) ms",
    "rest": r"rest-ready after (\d+) ms",
}
ENHANCED = re.compile(r"\[Enhanced\] stage=([a-z_]+) boot_ms=(\d+)")


def parse(text):
    legacy = {}
    for label, pattern in LEGACY.items():
        if match := re.search(pattern, text):
            legacy[label] = int(match[1])
    enhanced = {}
    for label, ms in ENHANCED.findall(text):
        enhanced.setdefault(label, int(ms))
    return legacy, enhanced


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("logs", type=Path, nargs="+")
    for path in cli.parse_args().logs:
        legacy, enhanced = parse(path.read_text(encoding="utf-8-sig", errors="replace"))
        print(path.name)
        for origin, milestones in (("game startup", legacy), ("userAppInit", enhanced)):
            for name, ms in milestones.items():
                print(f"  {origin:12} {name:28} {ms:>8} ms")
        if {"native_surface_released", "first_vulkan_frame"} <= enhanced.keys():
            gap = enhanced["first_vulkan_frame"] - enhanced["native_surface_released"]
            print(f"  Native buffer release to Vulkan present: {gap} ms")
        if not legacy and not enhanced:
            print("  No startup markers found")


if __name__ == "__main__":
    main()
