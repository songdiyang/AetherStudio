#!/usr/bin/env python3
"""Aether Studio GUI 自动化探测脚本 - 基于屏幕区域截图"""
import sys
import time
import json
import ctypes
import psutil
from pathlib import Path
from datetime import datetime

import uiautomation as auto
import mss

# 项目目录
PROJECT_DIR = Path(__file__).resolve().parent.parent
SCREENSHOT_DIR = PROJECT_DIR / "tests" / "screenshots"
SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)
REPORT_PATH = PROJECT_DIR / "tests" / "gui_probe_report.json"

user32 = ctypes.windll.user32
SW_RESTORE = 9


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", ctypes.c_long),
        ("top", ctypes.c_long),
        ("right", ctypes.c_long),
        ("bottom", ctypes.c_long),
    ]


def get_window_rect(hwnd):
    rect = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    return rect


def capture_window_region(hwnd, name):
    """使用 mss 截取窗口矩形区域（只包含该窗口）"""
    rect = get_window_rect(hwnd)
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    if width <= 0 or height <= 0:
        return None

    with mss.mss() as sct:
        monitor = {
            "left": rect.left,
            "top": rect.top,
            "width": width,
            "height": height,
        }
        img = sct.grab(monitor)
        path = SCREENSHOT_DIR / f"{name}_{datetime.now().strftime('%H%M%S')}.png"
        mss.tools.to_png(img.rgb, img.size, output=str(path))
        return str(path)


def restore_window(hwnd):
    """恢复最小化的窗口并置顶"""
    user32.ShowWindow.argtypes = [ctypes.c_void_p, ctypes.c_int]
    user32.ShowWindow.restype = ctypes.c_bool
    user32.ShowWindow(hwnd, SW_RESTORE)
    user32.SetForegroundWindow.argtypes = [ctypes.c_void_p]
    user32.SetForegroundWindow.restype = ctypes.c_bool
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.5)


def get_process(pid):
    try:
        return psutil.Process(pid)
    except psutil.NoSuchProcess:
        return None


def snapshot_perf(pid):
    proc = get_process(pid)
    if not proc:
        return None
    with proc.oneshot():
        return {
            "cpu_percent": proc.cpu_percent(interval=0.1),
            "memory_mb": proc.memory_info().rss / (1024 * 1024),
            "handles": proc.num_handles() if hasattr(proc, "num_handles") else None,
            "threads": proc.num_threads(),
        }


def enumerate_controls(control, depth=0, max_depth=4):
    if depth > max_depth:
        return []
    try:
        name = control.Name
        ctrl_type = control.ControlTypeName
        rect = control.BoundingRectangle
        info = {
            "type": ctrl_type,
            "name": name,
            "rect": {
                "left": rect.left if rect else None,
                "top": rect.top if rect else None,
                "right": rect.right if rect else None,
                "bottom": rect.bottom if rect else None,
            },
            "depth": depth,
        }
        children = []
        for child in control.GetChildren():
            children.extend(enumerate_controls(child, depth + 1, max_depth))
        return [info] + children
    except Exception as e:
        return [{"error": str(e), "depth": depth}]


def main():
    print("Searching for Aether window...")
    window = auto.WindowControl(searchDepth=1, Name="Aether")
    if not window.Exists(5):
        print("ERROR: Aether window not found")
        sys.exit(1)

    hwnd = window.NativeWindowHandle
    pid = window.ProcessId
    print(f"Found window: hwnd={hwnd}, pid={pid}")

    restore_window(hwnd)
    rect = get_window_rect(hwnd)
    print(f"Window rect after restore: ({rect.left},{rect.top},{rect.right},{rect.bottom})")

    shot1 = capture_window_region(hwnd, "initial")
    print(f"Screenshot: {shot1}")

    perf1 = snapshot_perf(pid)
    print(f"Initial perf: {perf1}")

    tree = enumerate_controls(window, max_depth=3)
    print(f"Found {len(tree)} controls")

    report = {
        "timestamp": datetime.now().isoformat(),
        "pid": pid,
        "hwnd": hwnd,
        "window_rect": {
            "left": rect.left,
            "top": rect.top,
            "right": rect.right,
            "bottom": rect.bottom,
        },
        "perf_initial": perf1,
        "screenshots": [shot1] if shot1 else [],
        "control_count": len(tree),
        "controls": tree[:50],
    }

    REPORT_PATH.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"Report saved: {REPORT_PATH}")


if __name__ == "__main__":
    main()
