//! Doc comments (specification, § 4.2): copied into the WIT (§ 13.1).

use waslc::{Package, SourceMap};

const PROGRAM: &str = r#"
/// The shop service.
world w {
  export shop;
}

/// Prices and orders.
/// Two lines.
interface shop {
  /// Cents.
  type price = u64;

  /// A line of an order.
  record order_line {
    /// The product.
    sku: string,
    quantity: u32,
  }

  /// Where an order is.
  enum state {
    /// Not paid yet.
    open,
    closed,
  }

  flags delivery {
    /// By email.
    email,
  }

  variant discount {
    none,
    /// A percentage.
    percent(u8),
  }

  /// A cart.
  resource cart {
    items: u32,
    /// An empty cart.
    constructor() { return cart { items: 0 }; }
    /// The number of items.
    func count() -> u32 { return self.items; }
    /// A cart with one item.
    static func one() -> cart { return cart { items: 1 }; }
  }

  // An ordinary comment.
  /// Adds the lines.
  ///
  ///   Indented.
  func total(lines: list<order_line>) -> price {
    /// Inside a body: an ordinary comment.
    local total: price = 0;
    return total;
  }

  /// Not a doc comment, since an ordinary comment follows.
  // Ordinary.
  func plain() { }

  //// Four slashes: ordinary.
  func four() { }
}
"#;

fn wit() -> String {
    let mut sources = SourceMap::default();
    sources.add("src/shop.wasl".into(), PROGRAM.to_string());
    let package = Package::new("test", "docs", "0.1.0").unwrap();
    let analysis = waslc::analyze(&package, &sources, &Default::default())
        .unwrap_or_else(|mut e| panic!("{}", e.remove(0).display(&sources)));
    analysis.wit().unwrap()
}

#[test]
fn doc_comments_are_copied_into_the_wit() {
    let wit = wit();
    for expected in [
        "/// The shop service.\nworld w {",
        "/// Prices and orders.\n/// Two lines.\ninterface shop {",
        "  /// Cents.\n  type price = u64;",
        "  /// A line of an order.\n  record order-line {\n    /// The product.\n    sku: string,\n    quantity: u32,",
        "  /// Where an order is.\n  enum state {\n    /// Not paid yet.\n    open,\n    closed,",
        "    /// By email.\n    email,",
        "    none,\n    /// A percentage.\n    percent(u8),",
        "  /// A cart.\n  resource cart {",
        "    /// An empty cart.\n    constructor();",
        "    /// The number of items.\n    count: func() -> u32;",
        "    /// A cart with one item.\n    one: static func() -> cart;",
        "  /// Adds the lines.\n  ///\n  ///   Indented.\n  total: func(",
        "  plain: func();",
        "  four: func();",
    ] {
        assert!(wit.contains(expected), "{expected}\n{wit}");
    }
    for absent in [
        "Inside a body",
        "Not a doc comment",
        "Ordinary",
        "Four slashes",
    ] {
        assert!(!wit.contains(absent), "{absent}\n{wit}");
    }
}
