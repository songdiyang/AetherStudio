#!/usr/bin/env python3
"""Aether Studio GUI Smoke Test - 基于坐标驱动的自动化测试"""
import sys
import time
import json
import ctypes
import shutil
import subprocess
from pathlib import Path
from datetime import datetime

import uiautomation as auto
import mss
import psutil

# 项目目录
PROJECT_DIR = Path(__file__).resolve().parent.parent
APP_PATH = PROJECT_DIR / "target" / "x86_64-pc-windows-msvc" / "release" / "aether-app.exe"
SCREENSHOT_DIR = PROJECT_DIR / "tests" / "screenshots"
HIT_REGIONS_PATH = PROJECT_DIR / "tests" / "gui_hit_regions.jsonl"
REPORT_PATH = PROJECT_DIR / "tests" / "gui_smoke_report.json"

SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)

user32 = ctypes.windll.user32
SW_RESTORE = 9
MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_ABSOLUTE = 0x8000
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004


class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


def get_window_rect(hwnd):
    rect = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    return rect


def restore_window(hwnd):
    user32.ShowWindow.argtypes = [ctypes.c_void_p, ctypes.c_int]
    user32.ShowWindow.restype = ctypes.c_bool
    user32.ShowWindow(hwnd, SW_RESTORE)
    user32.SetForegroundWindow.argtypes = [ctypes.c_void_p]
    user32.SetForegroundWindow.restype = ctypes.c_bool
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.5)


def capture_window_region(hwnd, name):
    rect = get_window_rect(hwnd)
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    if width <= 0 or height <= 0:
        return None
    with mss.mss() as sct:
        monitor = {"left": rect.left, "top": rect.top, "width": width, "height": height}
        img = sct.grab(monitor)
        path = SCREENSHOT_DIR / f"{name}_{datetime.now().strftime('%H%M%S')}.png"
        mss.tools.to_png(img.rgb, img.size, output=str(path))
        return str(path)


def click_at(hwnd, x, y):
    """在窗口客户区坐标 (x,y) 处点击"""
    rect = get_window_rect(hwnd)
    abs_x = int(rect.left + x)
    abs_y = int(rect.top + y)

    # 移动鼠标
    user32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
    user32.SetCursorPos(abs_x, abs_y)
    time.sleep(0.05)

    # 发送鼠标事件（使用绝对坐标 + 相对值）
    screen_w = user32.GetSystemMetrics(0)
    screen_h = user32.GetSystemMetrics(1)
    normalized_x = int(abs_x * 65535 / screen_w)
    normalized_y = int(abs_y * 65535 / screen_h)

    user32.mouse_event.argtypes = [ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_void_p]
    user32.mouse_event(MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE, normalized_x, normalized_y, 0, None)
    time.sleep(0.02)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, None)
    time.sleep(0.05)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, None)
    time.sleep(0.2)


def read_hit_regions():
    if not HIT_REGIONS_PATH.exists():
        return []
    lines = HIT_REGIONS_PATH.read_text(encoding="utf-8").strip().splitlines()
    regions = []
    for line in lines:
        if not line.strip():
            continue
        try:
            regions.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return regions


def find_region(regions, action_prefix):
    """按 action 前缀查找最后一个匹配的区域"""
    for r in reversed(regions):
        if r["action"].startswith(action_prefix):
            return r
    return None


def center(r):
    return r["x"] + r["width"] / 2, r["y"] + r["height"] / 2


def snapshot_perf(pid):
    try:
        proc = psutil.Process(pid)
        with proc.oneshot():
            return {
                "cpu_percent": proc.cpu_percent(interval=0.1),
                "memory_mb": proc.memory_info().rss / (1024 * 1024),
                "handles": getattr(proc, "num_handles", lambda: None)(),
                "threads": proc.num_threads(),
            }
    except Exception as e:
        return {"error": str(e)}


def start_app():
    # 清理旧日志
    HIT_REGIONS_PATH.unlink(missing_ok=True)
    proc = subprocess.Popen([str(APP_PATH)], cwd=str(PROJECT_DIR))
    return proc


def wait_for_window(timeout=30):
    start = time.time()
    while time.time() - start < timeout:
        window = auto.WindowControl(searchDepth=1, Name="Aether")
        if window.Exists(1):
            return window
        time.sleep(0.5)
    return None


def main():
    print("Starting Aether Studio...")
    proc = start_app()

    print("Waiting for window...")
    window = wait_for_window()
    if not window:
        print("ERROR: Window did not appear")
        proc.kill()
        sys.exit(1)

    hwnd = window.NativeWindowHandle
    pid = window.ProcessId
    print(f"Window ready: hwnd={hwnd}, pid={pid}")
    restore_window(hwnd)

    report = {
        "timestamp": datetime.now().isoformat(),
        "pid": pid,
        "steps": [],
        "perf": {},
    }

    # Step 0: 初始截图 + 性能
    time.sleep(1.0)
    shot0 = capture_window_region(hwnd, "step_00_initial")
    report["steps"].append({"name": "initial", "screenshot": shot0})
    report["perf"]["initial"] = snapshot_perf(pid)

    # Step 1: 点击活动栏 Explorer
    regions = read_hit_regions()
    if explorer := find_region(regions, "activity:"):
        cx, cy = center(explorer)
        print(f"Clicking activity button at ({cx}, {cy})")
        click_at(hwnd, cx, cy)
        time.sleep(0.5)
        shot1 = capture_window_region(hwnd, "step_01_activity_click")
        report["steps"].append({"name": "activity_click", "target": explorer["action"], "screenshot": shot1})

    # Step 2: 点击标题栏左侧边栏切换按钮
    regions = read_hit_regions()
    if sidebar_btn := find_region(regions, "titlebar:left_sidebar"):
        cx, cy = center(sidebar_btn)
        print(f"Clicking left sidebar toggle at ({cx}, {cy})")
        click_at(hwnd, cx, cy)
        time.sleep(0.5)
        shot2 = capture_window_region(hwnd, "step_02_sidebar_toggle")
        report["steps"].append({"name": "sidebar_toggle", "target": sidebar_btn["action"], "screenshot": shot2})

    # Step 3: 点击标题栏底部面板切换按钮
    regions = read_hit_regions()
    if bottom_btn := find_region(regions, "titlebar:bottom_panel"):
        cx, cy = center(bottom_btn)
        print(f"Clicking bottom panel toggle at ({cx}, {cy})")
        click_at(hwnd, cx, cy)
        time.sleep(0.5)
        shot3 = capture_window_region(hwnd, "step_03_bottom_panel_toggle")
        report["steps"].append({"name": "bottom_panel_toggle", "target": bottom_btn["action"], "screenshot": shot3})

    report["perf"]["final"] = snapshot_perf(pid)

    # 保存报告
    REPORT_PATH.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"Report saved: {REPORT_PATH}")

    # 关闭应用
    print("Closing Aether Studio...")
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()

    print("Smoke test completed. App closed.")


if __name__ == "__main__":
    main()
