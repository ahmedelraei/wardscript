# ward: tests/e2e/semantics.ward
import unittest

import semantics as s
from wardscript import PanicError, Thrown, runtime


class Semantics(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_integer_division_truncates(self):
        self.assertEqual(s.int_div(7, 2), 3)
        self.assertEqual(s.int_div(-7, 2), -3)
        self.assertEqual(s.int_rem(-7, 2), -1)
        self.assertEqual(s.int_rem(7, -2), 1)
        with self.assertRaises(PanicError):
            s.int_div(1, 0)

    def test_float_remainder_and_rounding(self):
        self.assertEqual(s.float_rem(-7.5, 2.0), -1.5)
        self.assertEqual(s.rounded(2.5), 3)
        self.assertEqual(s.rounded(-2.5), -3)
        self.assertEqual(s.float_div(1.0, 0.0), float("inf"))
        self.assertEqual(s.float_div(3.0, 2.0), 1.5)

    def test_names(self):
        self.assertEqual(s.shadowing(), 12)
        self.assertEqual(s.keywords(2), 5)

    def test_bools_print_like_wardscript(self):
        self.assertEqual(s.bools(True, False), "true false false")

    def test_comparisons_do_not_chain(self):
        self.assertTrue(s.compare(1, 1, True))
        self.assertFalse(s.compare(1, 2, True))

    def test_assignment_copies(self):
        p = s.Point(x=1, from_=2)
        self.assertEqual(s.moved(p), s.Point(x=100, from_=2))
        self.assertEqual(p.x, 1)

        boxes = [s.Box(item=s.Point(x=0, from_=0)), s.Box(item=s.Point(x=0, from_=0))]
        out = s.set_nested(boxes, 1, 7)
        self.assertEqual(out[1].item.x, 7)
        self.assertEqual(boxes[1].item.x, 0)
        with self.assertRaises(PanicError):
            s.set_nested(boxes, 5, 7)

        m = {"a": 1}
        self.assertEqual(s.set_key(m, "b", 2), {"a": 1, "b": 2})
        self.assertEqual(m, {"a": 1})
        self.assertEqual(s.sum_keys({"a": 1, "b": 2}), "ab")

    def test_nested_options_stay_distinct(self):
        self.assertEqual(s.depth(s.nested(None)), "some(none)")
        self.assertEqual(s.depth(s.nested(3)), "some(some(3))")
        self.assertEqual(s.depth(None), "none")
        self.assertEqual(s.nested(3), 3)

    def test_parsing_numbers(self):
        self.assertEqual(s.int_or(" 42 ", 0), 42)
        self.assertEqual(s.int_or("-7", 0), -7)
        for bad in ("", "4.2", "1e3", "0x10", "1_000", "9223372036854775808"):
            self.assertEqual(s.int_or(bad, 0), 0, bad)
        self.assertEqual(s.float_or("4.2", 0.0), 4.2)
        self.assertEqual(s.float_or("180", 0.0), 180.0)
        for bad in ("", ".5", "5.", "1e3", "nan", "inf"):
            self.assertEqual(s.float_or(bad, -1.0), -1.0, bad)

    def test_lists(self):
        self.assertEqual(s.get_or([1, 2], 1, 9), 2)
        self.assertEqual(s.get_or([1, 2], 2, 9), 9)
        self.assertEqual(s.get_or([1, 2], -1, 9), 9)
        self.assertEqual(s.at([1, 2], 1), 2)
        with self.assertRaises(PanicError):
            s.at([1, 2], -1)
        self.assertEqual(s.countdown(3), [3, 2, 1])
        self.assertEqual(s.early([1, -2, -3]), -2)
        self.assertEqual(s.early([1]), 0)

    def test_enums_and_match(self):
        self.assertEqual(s.area(s.Shape.Dot()), 0.0)
        self.assertEqual(s.area(s.Shape.Circle(2.0)), 12.0)
        self.assertEqual(s.area(s.Shape.Rect(2.0, 3.0)), 6.0)
        self.assertEqual(s.classify(0, "", True), "zero/empty/yes")
        self.assertEqual(s.classify(5, "x", False), "many/x/no")

    def test_exceptions(self):
        self.assertEqual(s.caught("ok!"), "ok 3")
        self.assertEqual(s.caught(""), "empty")
        self.assertEqual(s.caught("nope"), "bad: nope")
        with self.assertRaises(Thrown) as cm:
            s.thrown("nope")
        self.assertEqual(cm.exception.value, s.Failure.Bad("nope"))
        self.assertEqual(s.validated("abc"), "abc")
        self.assertEqual(s.validated("abcdef"), "validation failed: `short` rejected the value")

    def test_evaluation_order(self):
        said = []

        def say(x):
            said.append(x)
            return x

        runtime.configure(tools={"log": {"say": say}})
        self.assertEqual(s.order(True), "ab true")
        self.assertEqual(said, ["a", "b"])
        said.clear()
        self.assertEqual(s.order(False), "ac true")
        self.assertEqual(said, ["a", "c", "d"])

    def test_unit_functions_return_none(self):
        self.assertIsNone(s.noop(1))
        self.assertIsNone(s.noop(-1))


if __name__ == "__main__":
    unittest.main()
