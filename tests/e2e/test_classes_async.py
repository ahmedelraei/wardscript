# ward: tests/e2e/classes.ward --async
# In async code, objects are created with `await Class._new(...)`, since `__init__`
# can't await.
import asyncio
import unittest

import classes as c
from wardscript import Trusted, runtime


class ClassesAsync(unittest.TestCase):
    def tearDown(self):
        runtime.reset()

    def test_objects(self):
        async def main():
            self.assertEqual(await c.shared(), 2)
            quiet = await c.Agent._new(Trusted("quiet"))
            loud = await c.Shouter._new(Trusted("loud"), Trusted(True))
            out = await c.run_all(Trusted([quiet, loud]), "hello")
            self.assertEqual(out, ["quiet saw 1", "LOUD SAW 1"])
            self.assertEqual(await c.guarded(loud, 0), -1)

        asyncio.run(main())


if __name__ == "__main__":
    unittest.main()
