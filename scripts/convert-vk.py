#!/usr/bin/env python3
"""Convert a Barretenberg VK binary (3680 bytes, limb-encoded) to formats needed
by the Soroban UltraHonk verifier and co-noir keccak prover.

BB format (3680 bytes) — "poseidon2" / limb-encoded:
  3 × 32-byte big-endian headers: log_circuit_size, num_public_inputs, pub_inputs_offset
  28 × 128-byte G1 points: each as (x_lo, x_hi, y_lo, y_hi) with 32 bytes per limb

Soroban compact format (1760 bytes):
  4 × 8-byte big-endian u64 headers: circuit_size, log_circuit_size, public_inputs_size, pub_inputs_offset
  27 × 64-byte G1 points: each as (x, y) with 32 bytes per coordinate

co-noir keccak format (1888 bytes):
  3 × 32-byte big-endian headers: log_circuit_size, num_public_inputs, pub_inputs_offset
  28 × 64-byte G1 points: each as (x, y) with 32 bytes per coordinate

Usage:
  python3 convert-vk.py [--circuit NAME] <input_vk> <output_soroban> [<output_keccak>]

When --circuit is supplied, conversion fails if the VK public-input count does
not match that circuit. This prevents a valid VK from being registered under
the wrong circuit name.
"""

import argparse
import hashlib
import struct
from pathlib import Path


EXPECTED_PUBLIC_INPUTS = {
    "deal_valid": 20,
    "reveal_board_valid": 25,
    "showdown_valid": 27,
    "time_bank_valid": 6,
}


def combine_limbs(lo: bytes, hi: bytes) -> bytes:
    """Reconstruct a 32-byte big-endian coordinate from (lo136, hi) limb pair."""
    out = bytearray(32)
    out[0:15] = hi[17:32]   # upper 15 bytes from hi
    out[15:32] = lo[15:32]  # lower 17 bytes from lo
    return bytes(out)


def parse_bb_vk(data: bytes):
    """Parse a BB VK binary into headers and G1 points."""
    if len(data) != 3680:
        raise ValueError(f"Unexpected VK size: {len(data)} bytes (expected 3680)")

    log_circuit_size = int.from_bytes(data[0:32], "big")
    num_public_inputs = int.from_bytes(data[32:64], "big")
    pub_inputs_offset = int.from_bytes(data[64:96], "big")
    circuit_size = 1 << log_circuit_size

    points = []
    offset = 96
    for i in range(28):
        x_lo = data[offset:offset + 32]
        x_hi = data[offset + 32:offset + 64]
        y_lo = data[offset + 64:offset + 96]
        y_hi = data[offset + 96:offset + 128]
        x = combine_limbs(x_lo, x_hi)
        y = combine_limbs(y_lo, y_hi)
        points.append((x, y))
        offset += 128

    return log_circuit_size, num_public_inputs, pub_inputs_offset, circuit_size, points


def write_soroban_compact(output_path, log_circuit_size, num_public_inputs, pub_inputs_offset, circuit_size, points):
    """Write Soroban compact VK (1824 bytes): 4×u64 header + 28 G1 points."""
    out = bytearray()
    out += struct.pack(">Q", circuit_size)
    out += struct.pack(">Q", log_circuit_size)
    out += struct.pack(">Q", num_public_inputs)
    out += struct.pack(">Q", pub_inputs_offset)

    for i in range(28):  # All 28 precomputed entity commitments
        x, y = points[i]
        out += x
        out += y

    assert len(out) == 1824, f"Soroban output size mismatch: {len(out)} != 1824"
    Path(output_path).write_bytes(out)
    return len(out)


def write_keccak_vk(output_path, log_circuit_size, num_public_inputs, pub_inputs_offset, points):
    """Write co-noir keccak VK (1888 bytes): 3×32-byte header + 28 G1 points."""
    out = bytearray()
    out += log_circuit_size.to_bytes(32, "big")
    out += num_public_inputs.to_bytes(32, "big")
    out += pub_inputs_offset.to_bytes(32, "big")

    for i in range(28):  # All 28 points
        x, y = points[i]
        out += x
        out += y

    assert len(out) == 1888, f"Keccak output size mismatch: {len(out)} != 1888"
    Path(output_path).write_bytes(out)
    return len(out)


def validate_circuit_pair(circuit: str, num_public_inputs: int) -> None:
    expected = EXPECTED_PUBLIC_INPUTS.get(circuit)
    if expected is None:
        choices = ", ".join(sorted(EXPECTED_PUBLIC_INPUTS))
        raise ValueError(f"Unknown circuit '{circuit}' (expected one of: {choices})")
    if num_public_inputs != expected:
        raise ValueError(
            f"VK/circuit mismatch for {circuit}: VK has {num_public_inputs} "
            f"public inputs, expected {expected}"
        )


def convert_vk(input_path: str, output_soroban: str, output_keccak: str = None, circuit: str = None):
    data = Path(input_path).read_bytes()

    if len(data) == 1824:
        num_public_inputs = struct.unpack(">Q", data[16:24])[0]
        if circuit:
            validate_circuit_pair(circuit, num_public_inputs)
        print(f"  VK already in Soroban compact format ({len(data)} bytes), copying as-is.")
        Path(output_soroban).write_bytes(data)
        return

    log_cs, num_pi, pi_off, cs, points = parse_bb_vk(data)
    if circuit:
        validate_circuit_pair(circuit, num_pi)
    print(f"  log_circuit_size={log_cs}, circuit_size={cs}")
    print(f"  num_public_inputs={num_pi}, pub_inputs_offset={pi_off}")
    print(f"  content_sha256={hashlib.sha256(data).hexdigest()}")

    sz = write_soroban_compact(output_soroban, log_cs, num_pi, pi_off, cs, points)
    print(f"  Soroban compact: {len(data)} -> {sz} bytes ({output_soroban})")

    if output_keccak:
        sz = write_keccak_vk(output_keccak, log_cs, num_pi, pi_off, points)
        print(f"  co-noir keccak:  {len(data)} -> {sz} bytes ({output_keccak})")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--circuit", choices=sorted(EXPECTED_PUBLIC_INPUTS))
    parser.add_argument("input_vk")
    parser.add_argument("output_soroban")
    parser.add_argument("output_keccak", nargs="?")
    args = parser.parse_args()
    try:
        convert_vk(
            args.input_vk,
            args.output_soroban,
            args.output_keccak,
            args.circuit,
        )
    except ValueError as exc:
        parser.error(str(exc))
