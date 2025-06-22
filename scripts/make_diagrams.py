#!/usr/bin/env python3

import argparse
import json
import os
import os.path as P
import re
from dataclasses import dataclass
from typing import Any

import matplotlib.pyplot as plt
import matplotlib.ticker
import pandas as pd


@dataclass
class Args:
    criterion_dir: str
    bench: str
    outdir: str
    format: str
    estimate: str
    show_only: bool

    @staticmethod
    def parse() -> "Args":
        parser = argparse.ArgumentParser()
        parser.add_argument(
            "--criterion-dir",
            metavar="<path>",
            help="directory to criterion's output",
            default="target/criterion",
        )
        parser.add_argument(
            "--bench",
            metavar="<str>",
            help="name of ehe benchmark to parse estimates",
            required=True,
        )
        parser.add_argument(
            "--outdir",
            metavar="<path>",
            help="directory to write output diagrams",
            default="target/criterion",
        )
        parser.add_argument(
            "--format",
            metavar="<str>",
            help="format of output diagrams, can be jepg, svg, png, ...",
            default="svg",
        )
        parser.add_argument(
            "--estimate",
            metavar="<str>",
            help="which estimate to use",
            default="slope",
        )
        parser.add_argument(
            "--show-only",
            action="store_true",
            help="does nothing but shows the estimates",
        )
        return Args(**vars(parser.parse_args()))


@dataclass
class Report:
    name: str
    group: str
    idx: int
    size: str
    estimates: Any

    @staticmethod
    def parse(name: str, dir: str) -> "Report | None":
        mat = name_pattern.match(name)
        if mat is None:
            return
        with open(P.join(dir, "new/estimates.json")) as f:
            estimates = json.load(f)
        return Report(
            name=name,
            group=str(mat.group("group")),
            idx=int(mat.group("idx")),
            size=str(mat.group("size")),
            estimates=estimates,
        )


args: Args
name_pattern: re.Pattern[str]


def parse_reports(dir: str) -> list[Report]:
    reports: list[Report] = []
    for name in os.listdir(dir):
        report_dir = P.join(dir, name)
        report = Report.parse(name, report_dir)
        if report is not None:
            reports.append(report)
    return reports


def make_figure(df: pd.DataFrame, title: str, unit: str = "ns"):
    match unit:
        case "ns":
            pass
        case "us":
            df = df / 1000
        case _:
            raise ValueError(f"unsupported unit '{unit}'")
    df.plot(figsize=(10, 5))

    ax = plt.gca()
    ax.yaxis.set_major_locator(matplotlib.ticker.MaxNLocator(20))
    plt.xlabel("Buffer Size")
    plt.ylabel(f"Measurement ({unit})")

    plt.title(title)
    plt.savefig(P.join(args.outdir, f"{title}.{args.format}"))


def make_diagram(reports: list[Report]):
    groups: dict[str, list[Report]] = {}
    for report in reports:
        group = groups.setdefault(report.group, [])
        group.append(report)

    df = pd.DataFrame()
    for g, group in groups.items():
        group.sort(key=lambda r: r.idx)
        df[g] = [int(r.estimates[args.estimate]["point_estimate"]) for r in group]

    first = next(iter(groups.values()))
    index = [r.size for r in first]
    df.index = pd.Index(index)

    if args.show_only:
        print(df)
        return

    mid_l = int((len(first) - 5) / 2)
    mid_r = len(first) - mid_l

    make_figure(df[0:5], f"{args.bench}_first_5")
    make_figure(df[mid_l:mid_r], f"{args.bench}_mid_5", unit="us")
    make_figure(df[-5:], f"{args.bench}_last_5", unit="us")
    make_figure(df[:], f"{args.bench}_all", unit="us")


def main():
    global args, name_pattern
    args = Args.parse()

    name_pattern = re.compile(
        rf"^{args.bench}_(?P<idx>\d+)_(?P<size>\d+[A-Z])_(?P<group>\w+)$"
    )
    bench_dir = P.join(args.criterion_dir, args.bench)
    reports = parse_reports(bench_dir)

    os.makedirs(args.outdir, exist_ok=True)
    make_diagram(reports)


if __name__ == "__main__":
    main()
