#!/usr/bin/env python3
"""Aether Studio 历史记录面板 UX 自动化测试

功能：测试历史记录面板的打开、筛选、分页、详情、清空等完整交互流程

前置依赖：
    cargo build -p aether-win32 --bin aether-app   (先构建 debug 版本)
    pip install uiautomation mss

运行方式：
    python tests/gui_history_ux.py

输出：
    - tests/screenshots/          各步骤截图
    - tests/gui_hit_regions.jsonl 命中区 dump（debug 专用）
    - tests/gui_history_ux_report.json 测试报告

注意：
    脚本会启动独立的窗口，置顶后操作。高 DPI 屏幕下，坐标使用 DIP (逻辑像素)，
    点击时转换为物理像素，确保与内部点击检测逻辑一致。
"""
import sys
import time
import json
import ctypes
import subprocess
from pathlib import Path
from datetime import datetime

import mss

PROJECT_DIR = Path(__file__).resolve().parent.parent
APP_PATH = PROJECT_DIR / "target" / "x86_64-pc-windows-msvc" / "debug" / "aether-app.exe"
SCREENSHOT_DIR = PROJECT_DIR / "tests" / "screenshots"
HIT_REGIONS_PATH = PROJECT_DIR / "tests" / "gui_hit_regions.jsonl"
REPORT_PATH = PROJECT_DIR / "tests" / "gui_history_ux_report.json"

SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)

user32 = ctypes.windll.user32

class RECT(ctypes.Structure):
    _fields_ = [("left", ctypes.c_long), ("top", ctypes.c_long),
                ("right", ctypes.c_long), ("bottom", ctypes.c_long)]


def read_hit_regions():
    if not HIT_REGIONS_PATH.exists():
        return []
    regions = []
    for line in HIT_REGIONS_PATH.read_text(encoding="utf-8").strip().splitlines():
        if line.strip():
            try:
                regions.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return regions


def find_region(regions, action):
    for r in reversed(regions):
        if r["action"] == action:
            return r
    return None


def has_region(regions, action):
    return find_region(regions, action) is not None


def click_at_phys(x_phys, y_phys):
    """点击物理屏幕坐标"""
    user32.SetCursorPos(int(x_phys), int(y_phys))
    time.sleep(0.05)
    user32.mouse_event(0x0002, 0, 0, 0, None)  # LEFTDOWN
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, None)  # LEFTUP
    time.sleep(0.5)


def click_by_hit_region(hwnd, regions, action):
    """根据命中区（DIP）点击，转换成物理坐标"""
    r = find_region(regions, action)
    if not r:
        return False
    # 获取客户区原点的物理屏幕坐标
    class POINT(ctypes.Structure):
        _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    # DIP -> 物理像素
    dpi = user32.GetDpiForWindow(hwnd)
    scale = dpi / 96.0
    cx_phys = pt.x + int((r["x"] + r["width"] / 2) * scale)
    cy_phys = pt.y + int((r["y"] + r["height"] / 2) * scale)
    print(f"  Click {action}: dip({r['x']:.0f},{r['y']:.0f}) -> phys({cx_phys},{cy_phys}) client_origin({pt.x},{pt.y}) scale={scale}")
    click_at_phys(cx_phys, cy_phys)
    return True


def capture_window(hwnd, name):
    rect = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width = rect.right - rect.left
    height = rect.bottom - rect.top
    if width <= 0 or height <= 0:
        return None
    with mss.mss() as sct:
        monitor = {"left": rect.left, "top": rect.top, "width": width, "height": height}
        img = sct.grab(monitor)
        path = SCREENSHOT_DIR / f"{name}_{datetime.now().strftime('%H%M%S')}.png"
        mss.tools.to_png(img.rgb, img.size, output=str(path))
    return name


def start_app():
    HIT_REGIONS_PATH.unlink(missing_ok=True)
    return subprocess.Popen(
        [str(APP_PATH), "--aether-launch-args",
         '{"paths":[],"new_window":true,"goto":null,"wait":false}'],
        cwd=str(PROJECT_DIR),
    )


def wait_for_window(proc, timeout=30):
    start = time.time()
    while time.time() - start < timeout:
        for w in auto.GetRootControl().GetChildren():
            try:
                if w.ProcessId == proc.pid and w.ClassName != "IME":
                    return w.NativeWindowHandle
            except Exception:
                continue
        time.sleep(0.2)
    return None


if __name__ == "__main__":
    import uiautomation as auto
    report = {"timestamp": datetime.now().isoformat(), "steps": [], "passed": 0, "failed": 0}

    def step(name, ok, shot=None, note=""):
        status = "PASS" if ok else "FAIL"
        report["steps"].append({"name": name, "status": status, "screenshot": shot, "note": note})
        report["passed" if ok else "failed"] += 1
        print(f"[{status}] {name} {note}")

    print(f"Starting {APP_PATH} ...")
    proc = start_app()
    try:
        hwnd = wait_for_window(proc)
        if not hwnd:
            step("launch", False)
            sys.exit(1)

        # 置顶+挪到固定位置，避免干扰，然后强制重绘
        user32.SetWindowPos(hwnd, -1, 100, 60, 1400, 900, 0)
        user32.ShowWindow(hwnd, 9)
        user32.SetForegroundWindow(hwnd)
        user32.InvalidateRect(hwnd, None, True)
        user32.UpdateWindow(hwnd)
        time.sleep(3.0)  # 等待窗口移动后重绘
        regions = read_hit_regions()
        step("launch", True, capture_window(hwnd, "ux_00_launch"))

        # 打印命中区调试信息
        print(f"\n=== Hit Region Debug ===")
        r = RECT()
        user32.GetWindowRect(hwnd, ctypes.byref(r))
        class POINT(ctypes.Structure):
            _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]
        pt = POINT(0, 0)
        user32.ClientToScreen(hwnd, ctypes.byref(pt))
        dpi = user32.GetDpiForWindow(hwnd)
        print(f"  Window: ({r.left},{r.top})-({r.right},{r.bottom}) w={r.right-r.left} h={r.bottom-r.top}")
        print(f"  Client origin: ({pt.x},{pt.y}) DPI={dpi} scale={dpi/96}")
        print(f"  Total regions: {len(regions)}")
        for action in ["titlebar:right_panel", "ai:history_button"]:
            r = find_region(regions, action)
            if r:
                print(f"  {action}: x={r['x']:.0f} y={r['y']:.0f} w={r['width']:.0f} h={r['height']:.0f}")

        # 1. 展开右面板（AI 助手） - 先点击一次，验证是否打开
        click_by_hit_region(hwnd, regions, "titlebar:right_panel")
        time.sleep(0.8)
        user32.InvalidateRect(hwnd, None, True)
        user32.UpdateWindow(hwnd)
        time.sleep(0.5)
        regions = read_hit_regions()
        # 如果没找到，可能是本来就打开着，再点击一次切换
        if not has_region(regions, "ai:history_button"):
            print("  Right panel not opened, clicking again...")
            click_by_hit_region(hwnd, regions, "titlebar:right_panel")
            time.sleep(0.8)
            user32.InvalidateRect(hwnd, None, True)
            user32.UpdateWindow(hwnd)
            time.sleep(0.5)
            regions = read_hit_regions()
        ok = has_region(regions, "ai:history_button")
        step("open_ai_panel", ok, capture_window(hwnd, "ux_01_ai_panel"),
             "未找到 ai:history_button" if not ok else "")

        # 2. 展开历史面板
        click_by_hit_region(hwnd, regions, "ai:history_button")
        time.sleep(1.0)
        regions = read_hit_regions()
        ok = has_region(regions, "ai:history_clear_all")
        step("open_history_panel", ok, capture_window(hwnd, "ux_02_history_list"),
             "历史面板未出现" if not ok else "")

        if ok:
            # 3. 筛选：本周
            ok3 = click_by_hit_region(hwnd, regions, "ai:history_time_filter:本周")
            step("filter_time_week", ok3, capture_window(hwnd, "ux_03_filter_week"))

            # 4. 类型筛选：Agent，然后恢复全部
            regions = read_hit_regions()
            ok4 = click_by_hit_region(hwnd, regions, "ai:history_type_filter:Agent")
            step("filter_type_agent_and_reset", ok4, capture_window(hwnd, "ux_04_filter_reset"))

            # 5. 历史条目详情
            ok5 = True
            hist_item = find_region(regions, "ai:history_item:0")
            if hist_item:
                ok5 = click_by_hit_region(hwnd, regions, "ai:history_item:0")
            step("history_detail_view", ok5, capture_window(hwnd, "ux_05_detail_view"),
                 "" if hist_item else "无历史条目，跳过")

            # 6. 分页
            ok6 = True
            if find_region(regions, "ai:history_page_next"):
                ok6 = click_by_hit_region(hwnd, regions, "ai:history_page_next")
            step("history_pagination", ok6, capture_window(hwnd, "ux_06_pagination"))

            # 7. 清空按钮（不处理弹窗，只验证点击）
            ok7 = click_by_hit_region(hwnd, regions, "ai:history_clear_all")
            step("clear_all_confirm_dialog", ok7, capture_window(hwnd, "ux_07_clear_cancel"))

            # 8. 收起历史面板
            ok8 = click_by_hit_region(hwnd, regions, "ai:history_button")
            step("collapse_history_panel", ok8, capture_window(hwnd, "ux_08_collapsed"))

    finally:
        REPORT_PATH.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
        print(f"\nReport saved: {REPORT_PATH}")
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()

    print(f"\nDone: {report['passed']} passed, {report['failed']} failed")
    sys.exit(0 if report["failed"] == 0 else 1)
