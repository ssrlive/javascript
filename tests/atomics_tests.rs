use javascript::evaluate_script;

#[test]
fn atomics_methods_share_index_conversion() {
    let script = r#"
        const values = new Int32Array(new SharedArrayBuffer(4));
        let conversions = 0;
        const index = {
          valueOf() {
            conversions += 1;
            return 0;
          }
        };

        Atomics.store(values, index, 5);
        const loaded = Atomics.load(values, index);
        Atomics.waitAsync(values, index, 5, 0);
        let symbolErrors = 0;
        const operations = [
          () => Atomics.load(values, Symbol()),
          () => Atomics.store(values, Symbol(), 1),
          () => Atomics.add(values, Symbol(), 1),
          () => Atomics.compareExchange(values, Symbol(), 5, 6),
          () => Atomics.exchange(values, Symbol(), 1),
          () => Atomics.notify(values, Symbol()),
          () => Atomics.wait(values, Symbol(), 5, 0),
          () => Atomics.waitAsync(values, Symbol(), 5, 0)
        ];
        for (const operation of operations) {
          try {
            operation();
          } catch (error) {
            if (error instanceof TypeError) symbolErrors += 1;
          }
        }

        loaded === 5 && conversions === 3 && symbolErrors === operations.length
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn atomics_compare_exchange_preserves_index_error_order() {
    let script = r#"
        const values = new Int32Array(new SharedArrayBuffer(4));
        const order = [];
        const index = {
          valueOf() {
            order.push("index");
            return 1;
          }
        };
        let threwRangeError = false;

        try {
          Atomics.compareExchange(values, index, {
            valueOf() {
              order.push("expected");
              return 0;
            }
          }, 1);
        } catch (error) {
          threwRangeError = error instanceof RangeError;
        }

        threwRangeError && order.join(",") === "index"
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "true");
}
