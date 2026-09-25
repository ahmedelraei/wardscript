# ward: tests/e2e/classes.ward
import unittest

import classes as c
from wardscript import Trusted, TrustError, runtime
from wardscript.mock import MockModel


class Classes(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_objects_are_shared(self):
        self.assertEqual(c.shared(), 2)

    def test_virtual_and_super_calls(self):
        quiet = c.Agent(Trusted("quiet"))
        loud = c.Shouter(Trusted("loud"), Trusted(True))
        self.assertIsInstance(loud, c.Agent)
        out = c.run_all(Trusted([quiet, loud]), "hello")
        self.assertEqual(out, ["quiet saw 1", "LOUD SAW 1"])
        self.assertEqual(loud.calls(), 1)
        self.assertEqual(loud.notes, ["hello"])

    def test_fields_that_only_hold_trusted_data_need_vouching(self):
        with self.assertRaises(TrustError):
            c.Agent("unvouched")

    def test_ai_method_sees_the_object(self):
        model = MockModel({"summarize": "two notes"})
        runtime.configure(model=model)
        a = c.Agent(Trusted("a"))
        a.handle("first")
        a.handle("second")
        self.assertEqual(a.summarize(), "two notes")
        [request] = model.calls
        self.assertIn("first", request.prompt)
        self.assertIn("second", request.prompt)

    def test_method_that_throws(self):
        a = c.Agent(Trusted("a"))
        self.assertEqual(c.guarded(a, 5), 0)
        a.handle("x")
        a.handle("y")
        self.assertEqual(c.guarded(a, 1), -1)

    def test_interfaces_and_abstract_classes(self):
        self.assertEqual(c.roundtrip(), "store: kept")
        m = c.Memory()
        self.assertEqual(c.remember(Trusted(m), Trusted("x")), "store: x")
        with self.assertRaises(TrustError):
            c.remember(Trusted(m), "unvouched")
        self.assertEqual(c.names([m, m]), ["store", "store"])


if __name__ == "__main__":
    unittest.main()
