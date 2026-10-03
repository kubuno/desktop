// Compiles offline against the committed cache (kubuno-data/.sqlx, copied into trybuild's project).
kubuno_data::data_source!("../typed/shop.kbdata");

use kubuno_data::sqlx::SqlitePool;
use kubuno_data::TypedRow;

fn main() {
    let ada = Customer { id: 1, name: "Ada".into(), age: Some(36), ..Default::default() };
    assert_eq!(Customer::KEY, ["id"]);
    assert_eq!(Customer::from_values(&ada.to_values()).expect("round trip"), ada);
    assert_eq!(OrderTotalsRow::COLUMNS, ["customer_id", "total", "orders"]);
    // The data functions exist with these signatures (not run: no database here).
    let pool: Option<&SqlitePool> = None;
    if let Some(pool) = pool {
        let _all = Customer::fetch_all(pool);
        let _one = Customer::fetch_by_key(pool, 1);
        let _inserted = ada.insert(pool);
        let _updated = ada.update(pool);
        let _deleted = Customer::delete_by_key(pool, 1);
        let _view = VipCustomer::fetch_all(pool);
        let _older = customers_older_than(pool, 30);
        let _totals = order_totals(pool);
        let _vip = set_vip(pool, true, 1);
    }
}
