use surrealdb::RecordId;
use surrealdb_extras::SurrealQuery;

use crate::db::DBError;

#[derive(bon::Builder, SurrealQuery)]
#[query(
    check,
    error = DBError,
    sql = "RELATE {in_}->{table}->{out}"
)]
pub struct RelateQuery {
    #[builder(into)]
    in_: RecordId,
    #[builder(into)]
    table: surrealdb::sql::Table,
    #[builder(into)]
    out: RecordId,
}
