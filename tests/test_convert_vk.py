import importlib.util
import struct
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).parents[1] / "scripts" / "convert-vk.py"
SPEC = importlib.util.spec_from_file_location("convert_vk", SCRIPT)
convert_vk = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(convert_vk)


def compact_vk(public_inputs: int) -> bytes:
    header = struct.pack(">QQQQ", 1 << 10, 10, public_inputs, 0)
    return header + bytes(28 * 64)


class ConvertVkTests(unittest.TestCase):
    def test_compact_vk_accepts_matching_circuit(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "vk"
            output = Path(directory) / "vk.compact"
            source.write_bytes(compact_vk(20))

            convert_vk.convert_vk(str(source), str(output), circuit="deal_valid")

            self.assertEqual(output.read_bytes(), source.read_bytes())

    def test_compact_vk_rejects_mismatched_circuit(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "vk"
            output = Path(directory) / "vk.compact"
            source.write_bytes(compact_vk(27))

            with self.assertRaisesRegex(ValueError, "VK/circuit mismatch"):
                convert_vk.convert_vk(str(source), str(output), circuit="deal_valid")

            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
