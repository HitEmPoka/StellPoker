#!/usr/bin/env python3
"""Check the staging ResourceQuota / LimitRange still fit the Helm charts (Issue #674).

Worst case per chart = resources x max replicas (HPA maxReplicas when the chart
has an autoscaling block, else replicaCount), plus one surge pod per Deployment
for a RollingUpdate (25% maxSurge rounds up to 1 at these replica counts).
StatefulSets update in place, so mpc-node gets no surge pod.

Fails when:
  * the worst case exceeds any compute quota (requests/limits cpu+memory, pods)
  * a chart's container resources fall outside the LimitRange min/max or
    exceed maxLimitRequestRatio
  * a chart's PVC size falls outside the LimitRange PVC bounds or the storage
    quota

    python3 scripts/check_k8s_quota.py
"""

from __future__ import annotations

import re
import sys
from decimal import Decimal
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
HELM = ROOT / "infrastructure" / "helm"
STAGING = ROOT / "infrastructure" / "k8s" / "staging"

# chart -> workload kind (StatefulSets get no surge pod).
CHARTS = {
    "coordinator": "Deployment",
    "frontend": "Deployment",
    "mpc-node": "StatefulSet",
    "soroban-rpc-proxy": "Deployment",
}

_SUFFIX = {
    "m": Decimal("0.001"),
    "k": Decimal(10) ** 3,
    "M": Decimal(10) ** 6,
    "G": Decimal(10) ** 9,
    "T": Decimal(10) ** 12,
    "Ki": Decimal(2) ** 10,
    "Mi": Decimal(2) ** 20,
    "Gi": Decimal(2) ** 30,
    "Ti": Decimal(2) ** 40,
}


def qty(value) -> Decimal:
    """Parse a Kubernetes quantity ("500m", "2", "512Mi") into a Decimal."""
    m = re.fullmatch(r"([0-9.]+)([A-Za-z]*)", str(value).strip())
    if not m:
        raise ValueError(f"unparseable quantity: {value!r}")
    number, suffix = m.groups()
    if suffix and suffix not in _SUFFIX:
        raise ValueError(f"unknown quantity suffix in {value!r}")
    return Decimal(number) * _SUFFIX.get(suffix, Decimal(1))


def fmt(resource: str, value: Decimal) -> str:
    if resource == "cpu":
        return f"{value.normalize():f} cpu"
    return f"{(value / _SUFFIX['Gi']).quantize(Decimal('0.01'))}Gi"


def load_docs(path: Path) -> list[dict]:
    return [d for d in yaml.safe_load_all(path.read_text()) if d]


def main() -> int:
    errors: list[str] = []

    quota: dict[str, Decimal] = {}
    for doc in load_docs(STAGING / "resourcequota.yaml"):
        for key, value in doc["spec"]["hard"].items():
            quota[key] = qty(value)

    limit_range = load_docs(STAGING / "limitrange.yaml")[0]["spec"]["limits"]
    container_lr = next(item for item in limit_range if item["type"] == "Container")
    pvc_lr = next(item for item in limit_range if item["type"] == "PersistentVolumeClaim")

    totals = {
        "requests.cpu": Decimal(0),
        "requests.memory": Decimal(0),
        "limits.cpu": Decimal(0),
        "limits.memory": Decimal(0),
        "pods": Decimal(0),
    }
    storage = Decimal(0)
    pvcs = 0

    for chart, kind in CHARTS.items():
        values = yaml.safe_load((HELM / chart / "values.yaml").read_text())
        resources = values.get("resources") or {}
        requests = resources.get("requests") or {}
        limits = resources.get("limits") or {}

        autoscaling = values.get("autoscaling") or {}
        replicas = int(autoscaling.get("maxReplicas") or values.get("replicaCount", 1))
        pods = replicas + (1 if kind == "Deployment" else 0)
        totals["pods"] += pods

        for res in ("cpu", "memory"):
            # A missing request/limit falls back to the LimitRange default,
            # which is what the API server would inject.
            req = qty(requests.get(res, container_lr["defaultRequest"][res]))
            lim = qty(limits.get(res, container_lr["default"][res]))
            totals[f"requests.{res}"] += req * pods
            totals[f"limits.{res}"] += lim * pods

            lo, hi = qty(container_lr["min"][res]), qty(container_lr["max"][res])
            if req < lo:
                errors.append(f"{chart}: {res} request {fmt(res, req)} below LimitRange min {fmt(res, lo)}")
            if lim > hi:
                errors.append(f"{chart}: {res} limit {fmt(res, lim)} above LimitRange max {fmt(res, hi)}")
            ratio = qty(container_lr["maxLimitRequestRatio"][res])
            if req and lim / req > ratio:
                errors.append(
                    f"{chart}: {res} limit/request ratio {(lim / req).normalize():f} "
                    f"exceeds LimitRange maxLimitRequestRatio {ratio.normalize():f}"
                )

        persistence = values.get("persistence") or {}
        if persistence.get("enabled") and persistence.get("size"):
            size = qty(persistence["size"])
            storage += size
            pvcs += 1
            if not qty(pvc_lr["min"]["storage"]) <= size <= qty(pvc_lr["max"]["storage"]):
                errors.append(f"{chart}: PVC size {persistence['size']} outside LimitRange PVC bounds")

        print(f"  {chart:<18} {kind:<11} pods={pods}")

    for key, total in totals.items():
        hard = quota.get(key)
        if hard is None:
            errors.append(f"ResourceQuota is missing {key}")
            continue
        shown = int(total) if key == "pods" else fmt(key.split(".")[1], total)
        shown_hard = int(hard) if key == "pods" else fmt(key.split(".")[1], hard)
        print(f"  {key:<18} worst case {shown:<12} quota {shown_hard}")
        if total > hard:
            errors.append(f"worst case {key} {shown} exceeds quota {shown_hard}")

    if storage > quota.get("requests.storage", Decimal(0)):
        errors.append("chart PVCs exceed requests.storage quota")
    if pvcs > quota.get("persistentvolumeclaims", Decimal(0)):
        errors.append("chart PVC count exceeds persistentvolumeclaims quota")

    if errors:
        for e in errors:
            print(f"QUOTA: {e}", file=sys.stderr)
        print("Update infrastructure/k8s/staging/ or the chart values.", file=sys.stderr)
        return 1
    print("Staging quota and LimitRange cover the Helm charts' worst case.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
