#!/usr/bin/env python3
"""Sink profiles (sinks/*.md): make a skeleton from a session, check them, render the overview table.

Usage:
  sink-profile.py new [--log FILE] [--id ID]   skeleton of a profile on stdout, from the last `sink_caps` event of
                                               wfd-source (journal of sway-miracast-source, or FILE with the JSON
                                               lines) and the sink's P2P advertisement (wpa_supplicant journal)
  sink-profile.py check                        validate every profile against sinks/checklist.yaml and check
                                               that the table in sinks/README.md is current (make lint)
  sink-profile.py table                        rewrite the table in sinks/README.md

A full skeleton needs a session with WFD_PROBE_CAPS=1 (wfd-source asks for every known capability); without it
only the four parameters of a normal M3 are known. No MAC address or TV name is written: wfd-source already
redacts them, and the P2P part keeps only the device type and the WFD device info. Needs PyYAML.
"""

import argparse
import datetime
import json
import pathlib
import re
import subprocess
import sys

import yaml

ROOT = pathlib.Path(__file__).resolve().parent.parent
SINKS = ROOT / "sinks"
CHECKLIST = SINKS / "checklist.yaml"
README = SINKS / "README.md"
TABLE_BEGIN = "<!-- profiles:begin (tools/sink-profile.py table) -->"
TABLE_END = "<!-- profiles:end -->"
UNIT = "sway-miracast-source.service"
REQUIRED = ("id", "vendor", "model", "platform", "identify", "announced", "checklist")


def load_checklist():
    data = yaml.safe_load(CHECKLIST.read_text())
    items = {k: (group, q) for group, qs in data["groups"].items() for k, q in qs.items()}
    return data["statuses"], items


def front_matter(path):
    """(front matter dict, body) of a profile; ValueError when the file does not start with one."""
    text = path.read_text()
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        raise ValueError("no YAML front matter (--- ... ---) at the top")
    data = yaml.safe_load(m.group(1))
    if not isinstance(data, dict):
        raise ValueError("front matter is not a mapping")
    return data, text[m.end() :]


def profiles():
    return sorted(p for p in SINKS.glob("*.md") if p.name != "README.md")


# --- new -----------------------------------------------------------------------------------------------------


def journal(args):
    out = subprocess.run(["journalctl", "--no-pager", "-o", "cat", *args], capture_output=True, text=True)
    return out.stdout.splitlines()


def events(lines):
    for line in lines:
        line = line.strip()
        if line.startswith("{"):
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                pass


def p2p_info(mac):
    """pri_dev_type and wfd_dev_info of the sink from wpa_supplicant's last P2P-DEVICE-FOUND (no name, no MAC)."""
    if not mac:
        return {}
    found = [l for l in journal(["-t", "wpa_supplicant"]) if f"P2P-DEVICE-FOUND {mac.lower()}" in l.lower()]
    if not found:
        return {}
    info = {}
    for key in ("pri_dev_type", "wfd_dev_info"):
        m = re.search(rf"\b{key}=(\S+)", found[-1])
        if m:
            info[key] = m.group(1)
    if "wfd_dev_info" in info:
        info["wfd_dev_info"] = "0x" + info["wfd_dev_info"].removeprefix("0x")[:4]
    return info


def parse_params(text):
    announced, notes = {}, []
    for line in text.splitlines():
        if line.startswith("#"):
            notes.append(line.lstrip("# ").strip())
        elif ":" in line:
            name, value = line.split(":", 1)
            announced[name.strip()] = value.strip()
    return announced, notes


def skeleton(evs, ident):
    caps = [e for e in evs if e.get("event") == "sink_caps"]
    if not caps:
        sys.exit("no sink_caps event: cast to the TV once (WFD_PROBE_CAPS=1 for the full list), then run again")
    last = caps[-1]
    starts = [e for e in evs if e.get("event") == "start"]
    mac = starts[-1].get("sink", "") if starts else ""
    announced, notes = parse_params(last.get("params", ""))
    _, items = load_checklist()
    today = datetime.date.today().isoformat()
    vendor_ext = sorted(
        n for n in announced if n.startswith(("wfd_sec_", "wfd_vnd_")) or not n.startswith(("wfd_", "wfd2_"))
    )
    checklist = {}
    for key in items:
        checklist[key] = {"value": "unknown", "status": "open"}
    if vendor_ext:
        checklist["vendor_extensions"] = {"value": vendor_ext, "status": "observed", "date": today}
    data = {
        "id": ident,
        "vendor": "TODO",
        "model": "TODO (as sold)",
        "platform": "TODO (OS and year)",
        "firmware": "unknown",
        "identify": {"user_agent": last.get("user_agent", ""), **p2p_info(mac)},
        "announced": announced,
        "checklist": checklist,
    }
    head = yaml.safe_dump(data, sort_keys=False, allow_unicode=True, width=120)
    probe = "full probe (WFD_PROBE_CAPS=1)" if last.get("probe") else "normal M3 only (set WFD_PROBE_CAPS=1 for all)"
    body = [
        f"# {ident}",
        "",
        f"Announced capabilities: from wfd-source event `sink_caps`, {probe}, {today}.",
    ]
    if notes:
        body.append("Groups the sink did not answer: " + "; ".join(notes) + ".")
    body += [
        "",
        "Source settings: TODO (mode, bitrate, what M4 needs).",
        "",
        "## Behaviours",
        "",
        "### Q1: <short name>",
        "- Observation:",
        "- Evidence: (log or measurement, path, date)",
        "- Reproduce: (how to trigger it on purpose)",
        "- Specification: (what it says, or 'none')",
        "- Workaround: (where in the code). Status: confirmed | hypothesis | workaround in code | open",
        "",
        "## Open",
        "",
    ]
    return f"---\n{head}---\n" + "\n".join(body)


def cmd_new(a):
    lines = pathlib.Path(a.log).read_text().splitlines() if a.log else journal(["--user", "-u", UNIT])
    print(skeleton(list(events(lines)), a.id), end="")


# --- check and table -----------------------------------------------------------------------------------------


def problems(path, statuses, items):
    try:
        data, _ = front_matter(path)
    except (ValueError, yaml.YAMLError) as e:
        return [str(e)]
    errs = [f"missing `{k}`" for k in REQUIRED if k not in data]
    if data.get("id") not in (None, path.stem):
        errs.append(f"id `{data.get('id')}` is not the file name `{path.stem}`")
    if not isinstance(data.get("identify", {}), dict) or "user_agent" not in data.get("identify", {}):
        errs.append("`identify` needs `user_agent`")
    if not isinstance(data.get("announced", {}), dict):
        errs.append("`announced` must map parameter names to the sink's raw values")
    checklist = data.get("checklist") or {}
    if not isinstance(checklist, dict):
        return errs + ["`checklist` must be a mapping"]
    errs += [f"checklist: unknown item `{k}` (add it to sinks/checklist.yaml)" for k in checklist if k not in items]
    errs += [f"checklist: missing item `{k}`" for k in items if k not in checklist]
    for k, v in checklist.items():
        if not isinstance(v, dict) or "value" not in v or "status" not in v:
            errs.append(f"checklist.{k}: needs `value` and `status`")
        elif v["status"] not in statuses:
            errs.append(f"checklist.{k}: status `{v['status']}` not in {statuses}")
        elif v["status"] == "open" and v["value"] not in ("unknown", None):
            errs.append(f"checklist.{k}: status open but a value is given (use observed)")
    return errs


def cell(item):
    if not item or item.get("status") == "open":
        return "?"
    v = item["value"]
    v = ", ".join(map(str, v)) if isinstance(v, list) else str(v)
    return v + ("" if item["status"] == "confirmed" else " (observed)")


def table():
    rows = [
        "| Profile | Platform | RTSP User-Agent | Wakes from standby | TEARDOWN without video (s) | Glass to glass (ms) | Open items |",
        "|---|---|---|---|---|---|---|",
    ]
    for p in profiles():
        d, _ = front_matter(p)
        c = d.get("checklist", {})
        open_items = sum(1 for v in c.values() if isinstance(v, dict) and v.get("status") == "open")
        rows.append(
            f"| [{p.stem}]({p.name}) | {d.get('platform', '')} | `{d.get('identify', {}).get('user_agent', '')}` "
            f"| {cell(c.get('wake_from_standby'))} | {cell(c.get('teardown_without_video_s'))} "
            f"| {cell(c.get('glass_to_glass_ms'))} | {open_items} of {len(c)} |"
        )
    return "\n".join(rows)


def readme_with_table():
    text = README.read_text()
    m = re.search(re.escape(TABLE_BEGIN) + r"\n.*?" + re.escape(TABLE_END), text, re.S)
    if not m:
        sys.exit(f"{README}: markers {TABLE_BEGIN} ... {TABLE_END} not found")
    return text, text[: m.start()] + f"{TABLE_BEGIN}\n{table()}\n{TABLE_END}" + text[m.end() :]


def cmd_check(_a):
    statuses, items = load_checklist()
    bad = 0
    for p in profiles():
        for e in problems(p, statuses, items):
            print(f"{p.relative_to(ROOT)}: {e}", file=sys.stderr)
            bad += 1
    if not bad:
        old, new = readme_with_table()
        if old != new:
            print("sinks/README.md: profile table is out of date, run: tools/sink-profile.py table", file=sys.stderr)
            bad += 1
    sys.exit(1 if bad else 0)


def cmd_table(_a):
    _, new = readme_with_table()
    README.write_text(new)


def main():
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    n = sub.add_parser("new", help="skeleton of a profile from the last session")
    n.add_argument("--log", help="JSON lines of wfd-source instead of the journal")
    n.add_argument("--id", default="vendor-platform-model", help="profile id = file name without .md")
    n.set_defaults(fn=cmd_new)
    sub.add_parser("check", help="validate profiles and the README table").set_defaults(fn=cmd_check)
    sub.add_parser("table", help="rewrite the table in sinks/README.md").set_defaults(fn=cmd_table)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
