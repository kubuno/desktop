// Offline, a statement the committed cache does not have (a column added to the .kbdata since the
// cache was prepared) is a compile error that names the file and the statement.
kubuno_desktop_data::data_source!("fail_not_cached.kbdata");

fn main() {}
