"""Offline packet reconstruction from SEBasicLighting64.dll; performs no I/O.

This is research evidence, not the app's backend or a verified c615 sequence.
See ../installer-analysis.md for the source addresses and unresolved selection.
"""

import struct


def report(command, payload):
    if len(payload) > 956:
        raise ValueError("payload exceeds the 960-byte feature buffer")
    return (struct.pack("<BBH", 7, command, len(payload)) + payload).ljust(960, b"\0")


def autonomous(enabled):
    """AA wire value 1 = keyboard autonomous, 0 = Dynamic Lighting."""
    # Lenovo declares two payload bytes but only sets the first; the other is 0.
    return report(0xAA, bytes((int(enabled), 0)))


def chip_control(opcode, profile):
    """D0 builder at 0x18009a900; policy selects the arguments elsewhere."""
    return report(0xD0, bytes((opcode, profile)))


def live_colour(command, entries):
    """Build the observed A1/A8 item format, without guessing device mapping.

    entries: (wire_id, r, g, b). A1 uses one-byte IDs; A8 uses LE u16 IDs.
    Which IDs illuminate c615's physical keyboard still needs verification.
    """
    if command not in (0xA1, 0xA8):
        raise ValueError("live colour opcode must be A1 or A8")
    fmt = "<BBBB" if command == 0xA1 else "<HBBB"
    return report(command, b"".join(struct.pack(fmt, *entry) for entry in entries))


if __name__ == "__main__":
    examples = {
        "keyboard autonomous": autonomous(True),
        "Dynamic Lighting": autonomous(False),
        "D0 opcode 04 profile 2 (argument example)": chip_control(4, 2),
        "A8 ID 1501 blue (format example, mapping unverified)":
            live_colour(0xA8, [(1501, 0, 0, 255)]),
    }
    for label, packet in examples.items():
        print(f"{label}: {packet[:12].hex(' ')} ... ({len(packet)} bytes)")


def static_colour(profile, r, g, b):
    """Recovered OneZone CB: static animation 11, one colour, LED ID 1."""
    group = bytes.fromhex('01 06 01 0b 02 00 03 00 04 04 05 02 06 00')
    group += bytes((1, r, g, b, 1, 1, 0))
    return report(0xCB, bytes((profile, 1, 1)) + group)
