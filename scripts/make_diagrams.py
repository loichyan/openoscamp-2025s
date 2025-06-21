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
estimate_key = "slope"


def parse_reports(dir: str) -> list[Report]:
    reports: list[Report] = []
    for name in os.listdir(dir):
        report_dir = P.join(dir, name)
        report = Report.parse(name, report_dir)
        if report is not None:
            reports.append(report)
    return reports


def make_figure(df: pd.DataFrame, title: str, unit: str = "ns"):
    df.plot()

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

    first = next(iter(groups.values()))
    indices = [r.size for r in first]
    df = pd.DataFrame(index=indices)

    for g, group in groups.items():
        group.sort(key=lambda r: r.idx)
        df[g] = [r.estimates[estimate_key]["point_estimate"] for r in group]

    make_figure(df[0:5], f"{args.bench}_first_5")
    make_figure(df[-5:] / 1000, f"{args.bench}_last_5", unit="us")
    make_figure(df[:] / 1000, f"{args.bench}_all", unit="us")


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
