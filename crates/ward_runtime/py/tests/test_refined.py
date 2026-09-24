import unittest

from wardscript import DecodeError, PanicError, _rt, decode, json_schema


def short(s):
    return len(s) <= 3


class Refined(unittest.TestCase):
    def test_decode_checks_the_condition(self):
        t = _rt.Refined(_rt.String, short, "it.len() <= 3", {"maxLength": 3})
        self.assertEqual(decode(t, "abc"), "abc")
        with self.assertRaises(DecodeError) as cm:
            decode(_rt.List(t), ["ok", "too long"])
        self.assertEqual(cm.exception.path, "$[1]")
        self.assertIn("it.len() <= 3", str(cm.exception))
        # The base type is checked first.
        with self.assertRaises(DecodeError):
            decode(t, 3)

    def test_a_failing_condition_is_a_decode_error(self):
        def boom(_):
            raise PanicError("index out of bounds")

        with self.assertRaises(DecodeError) as cm:
            decode(_rt.Refined(_rt.Int, boom, "it[3] > 0"), 1)
        self.assertIn("index out of bounds", str(cm.exception))

    def test_schema(self):
        t = _rt.Refined(_rt.Int, lambda x: 1 <= x <= 5, "it >= 1 && it <= 5", {"minimum": 1, "maximum": 5})
        self.assertEqual(
            json_schema(t),
            {"type": "integer", "minimum": 1, "maximum": 5, "description": "must satisfy: it >= 1 && it <= 5"},
        )
        self.assertEqual(json_schema(_rt.Option(t))["anyOf"][0]["maximum"], 5)
        self.assertEqual(t, _rt.Refined(_rt.Int, short, "it >= 1 && it <= 5"))
        self.assertEqual(repr(t), "Int where it >= 1 && it <= 5")


if __name__ == "__main__":
    unittest.main()
