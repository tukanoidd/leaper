use surrealdb::RecordId;
use surrealdb_extras::SurrealQuery;

use crate::db::DBError;

#[derive(bon::Builder, SurrealQuery)]
#[query(check, error = DBError)]
pub struct RelateQuery {
    #[builder(into)]
    #[var(sql = "RELATE {}->")]
    in_: RecordId,
    #[builder(into)]
    #[var(sql = "{}->")]
    table: surrealdb::sql::Table,
    #[builder(into)]
    #[var(sql = "{}")]
    out: RecordId,
}

#[derive(bon::Builder, SurrealQuery)]
#[query(output = "Option<RecordId>", error = DBError)]
pub struct CreateEmptyIdQuery {
    #[builder(into)]
    #[var(sql = "(CREATE {}).id")]
    table: surrealdb::sql::Table,
}
