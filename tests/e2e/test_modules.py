# ward: tests/e2e/modules/main.wardscript
import unittest

import main
from shop import orders


class Modules(unittest.TestCase):
    def test_cross_module_calls(self):
        items = [orders.Item(name="a", size=orders.Size.Small, count=3), orders.Item(name="b", size=orders.Size.Large, count=1)]
        self.assertEqual(main.total(items), 11)

    def test_cross_module_values(self):
        item = main.restock(orders.Item(name="a", size=orders.Size.Small, count=1))
        self.assertEqual(item, orders.Item(name="a", size=orders.Size.Large, count=2))


if __name__ == "__main__":
    unittest.main()
