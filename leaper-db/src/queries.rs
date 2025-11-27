use surrealdb::types::RecordId;
use surrealdb_extras::SurrealQuery;

use crate::DBError;

#[derive(Debug, bon::Builder, SurrealQuery)]
#[query(
    check,
    error = DBError,
    sql = "RELATE {in_}->{table}->{out}"
)]
pub struct RelateQuery {
    #[builder(into)]
    in_: RecordId,
    #[builder(into)]
    table: surrealdb::types::Table,
    #[builder(into)]
    out: RecordId,
}
