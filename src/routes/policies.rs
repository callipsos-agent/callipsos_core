use axum::extract::{State, Query, Path};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::policy::PolicyRow;
use crate::db::user::User;
use crate::error::AppError;
use crate::policy::presets;
use crate::policy::rules::PolicyRule;
use crate::policy::types::UserId;
use crate::routes::AppState;

#[derive(Deserialize)]
pub struct CreatePolicyRequest {
    pub user_id: Uuid,
    pub name: String,
    pub preset: Option<String>,
    pub rules: Option<serde_json::Value>,
}

pub async fn create_policy(
    State(state): State<AppState>,
    Json(req): Json<CreatePolicyRequest>,
) -> Result<(StatusCode, Json<PolicyRow>), AppError> {
    // 1. Validate mutual exclusivity
    let rules_json = match (&req.preset, &req.rules) {
        (Some(_), Some(_)) => {
            return Err(AppError::BadRequest(
                "Provide either 'preset' or 'rules', not both".into(),
            ));
        }
        (None, None) => {
            return Err(AppError::BadRequest(
                "Provide either 'preset' or 'rules'".into(),
            ));
        }

        // 2. Preset path
        (Some(preset), None) => {
            let rules = match preset.as_str() {
                "safety_first" => presets::safety_first(),
                "balanced" => presets::balanced(),
                "best_yields" => presets::best_yields(),
                _ => {
                    return Err(AppError::BadRequest(format!(
                        "Invalid preset name: '{}'. Valid presets: safety_first, balanced, best_yields",
                        preset
                    )));
                }
            };
            serde_json::to_value(&rules).map_err(|e| AppError::Internal(e.to_string()))?
        }

        // 3. Custom rules path
        (None, Some(raw)) => {
            // Validate that it deserializes as Vec<PolicyRule>
            serde_json::from_value::<Vec<PolicyRule>>(raw.clone()).map_err(|e| {
                AppError::BadRequest(format!("Invalid rules: {e}"))
            })?;
            raw.clone()
        }
    };

    // 4. Verify user exists
    let user_id = UserId::from(req.user_id);
    let user = User::find_by_id(&state.db, user_id).await?;
    if user.is_none() {
        return Err(AppError::NotFound(format!("User {} not found", req.user_id)));
    }

    // 5. Insert
    let policy = PolicyRow::create(&state.db, user_id, &req.name, rules_json).await?;

    // 6. Return
    Ok((StatusCode::CREATED, Json(policy)))
}

// ── GET /api/v1/policies?user_id=<uuid> ─────────────────────

#[derive(Deserialize)]
pub struct GetPoliciesQuery {
    pub user_id: Uuid,
}

pub async fn get_policies(
    State(state): State<AppState>,
    Query(query): Query<GetPoliciesQuery>,
) -> Result<Json<Vec<PolicyRow>>, AppError> {
    let user_id = UserId::from(query.user_id);
    let policies = PolicyRow::find_active_by_user(&state.db, user_id).await?;
    Ok(Json(policies))
}

// ── DELETE /api/v1/policies/:id ─────────────────────────────

pub async fn delete_policy(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let deleted = PolicyRow::soft_delete(&state.db, id).await?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound(format!("Policy {} not found", id)))
    }
}






