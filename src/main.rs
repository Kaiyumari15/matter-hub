// Contains the main application logic for the server and data strutures

use std::{collections::HashMap, process::Command};

// --- Imports ---
use axum::{
    self,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use regex::Regex;
use serde::{Deserialize};
use sqlx::SqlitePool;
use tokio;
use dotenv;

// --- Structs ---

/// Application state incuding the database connection pool
///
/// ### Fields:
/// - db_pool: sqlx::SqlitePool - Connection pool for SQLite database
///
/// ### Derives:
/// - Clone: Enables cloning of AppState instances
///
/// ---
///
/// ### Example:
/// ```
/// let state = AppState {
///   db_pool: SqlitePool::connect("sqlite::memory:").await.unwrap(),
/// };
/// ```
#[derive(Clone)]
struct AppState {
    db_pool: sqlx::SqlitePool,
}

///  Represents a row in the devices table
///
/// ### Fields:
/// - id: i32 - Identifier for the device, unique only for this hub
/// - node_id: i32 - The unique node identifier associated with the device, unique for this network
/// - endpoint_id: i32 - Endpoint identifier for the device
/// - name: String - Name of the device
/// - capabilities: Json<HashMap<String, Vec<String>>> - JSON field representing the device's capabilities
///
/// ### Derives:
/// - Debug: Enables formatting using the {:?} formatter
/// - Clone: Enables cloning of DeviceRow instances
/// - sqlx::FromRow: Enables mapping from database rows to DeviceRow instances
///
/// ---
///
/// ### Example:
/// ```
/// let device = DeviceRow {
///   id: 1,
///   node_id: 1234
///   endpoint_id: 1,
///   name: "Living Room Light".to_string(),
///   capabilities: sqlx::types::Json(HashMap::new()),
/// };
#[derive(Debug, Clone, sqlx::FromRow)]
struct DeviceRow {
    #[allow(dead_code)]
    id: i32,
    node_id: i32,
    endpoint_id: i32,
    name: String,
    capabilities: sqlx::types::Json<HashMap<String, Vec<String>>>,
}

/// Represents a command request to be sent to a device
///
/// ### Fields:
/// - cluster: String - The cluster to which the command belongs
/// - command: String - The command to be executed
/// - args: Vec<String> - A list of arguments for the command
///
/// ### Derives:
/// - Debug: Enables formatting using the {:?} formatter
/// - Deserialize: Enables deserialization from formats like JSON
///
/// ---
///
/// ### Example:
/// ```
/// let request = CommandRequest {
///   cluster: "onoff".to_string(),
///   command: "toggle".to_string(),
///   args: vec![],
/// };
/// ```
#[derive(Debug, Deserialize)]
struct CommandRequest {
    cluster: String,
    command: String,
    args: Vec<String>
}

/// Represents a response after executing a command on a device
///
/// ### Fields:
/// - success: bool - Indicates if the command was executed successfully
/// - message: String - A message providing additional information about the command execution
///
/// ### Derives:
/// - Debug: Enables formatting using the {:?} formatter
/// - Serialize: Enables serialization to formats like JSON
///
/// ---
///
/// ### Example:
/// ```
/// let response = CommandResponse {
///   success: true,
///   message: "Command executed successfully".to_string(),
/// };
/// ```
#[derive(Debug, serde::Serialize)]
struct CommandResponse {
    success: bool,
    message: String,
}

/// Represents a request to commission a new device
///
/// ### Fields:
/// - pairing_code: i32 - The pairing code used for commissioning
/// - name: String - The user's name for the device
///
/// ### Derives:
/// - Debug: Enables formatting using the {:?} formatter
/// - Deserialize: Enables deserialization from formats like JSON
#[derive(Debug, Deserialize)]
struct CommissionRequest {
    pairing_code: i32,
    name: String,
}

/// Represents a response after attempting to commission a new device
///
/// ### Fields:
/// - success: bool - Indicates if the commissioning was successful
/// - id: Option<i32> - The ID of the newly commissioned device, if successful
/// - message: String - A message providing additional information about the commissioning process
///
/// ### Derives:
/// - Debug: Enables formatting using the {:?} formatter
/// - Serialize: Enables serialization to formats like JSON
///
/// ---
///
/// ### Example:
/// ```
/// let response = CommisionResponse {
///  success: true,
/// id: Some(1),
/// message: "Device commissioned successfully".to_string(),
/// };
/// ```
#[derive(Debug, serde::Serialize)]
struct CommissionResponse {
    success: bool,
    id: Option<i32>,
    message: String,
}

// --- Main Function ---
#[tokio::main]
async fn main() {
    // Initialize the database connection pool
    dotenv::dotenv().ok();
    let database_url = dotenv::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db_pool = SqlitePool::connect(&database_url)
        .await
        .expect("Failed to connect to the database");
    // Create application state
    let app_state = AppState { db_pool };
    // Initialize and run the Axum server
    let app = axum::Router::new()
        .route(
            "/devices/:node_id/:endpoint_id/command",
            axum::routing::post(handle_device_command),
        )
        .route(
            "/devices/commission",
            axum::routing::post(handle_device_commission),
        )
        .with_state(app_state);

    println!("Server running on http://localhost:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

// --- Handlers ---

/// Handles device command requests
///
/// This function processes incoming HTTP requests to execute commands on devices.
/// It should not be called directly; It is invoked by the Axum framework when a request is received.
///
/// ### Parameters:
/// - Path(id): Path<i32> - So the device ID can be extracted from the URL path
/// - State(state): State<AppState> - To access the database pool
/// - Json(payload): Json<CommandRequest> - The JSON body contains the command request details
///
/// ### Returns:
/// - impl IntoResponse - An HTTP response indicating the result of the command execution
async fn handle_device_command(
    Path(id): Path<i32>,
    State(state): State<AppState>,
    Json(payload): Json<CommandRequest>,
) -> impl IntoResponse {
    // Prepare the database query
    let db_pool = &state.db_pool;
    let query = sqlx::query_as::<_, DeviceRow>("SELECT * FROM devices WHERE id = ?").bind(id);

    // Fetch the device row from the database
    let row = query.fetch_one(db_pool).await;
    let device = match row {
        Ok(device) => device,
        Err(sqlx::Error::RowNotFound) => {
            let response = CommandResponse {
                success: false,
                message: format!("Device with id '{}' not found", id),
            };
            return (StatusCode::NOT_FOUND, Json(response));
        }
        Err(e) => {
            let response = CommandResponse {
                success: false,
                message: format!("Database error: {}", e),
            };
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(response));
        }
    };
    
    // Check the cluster is supported by this device
    let cluster = payload.cluster;
    let command = payload.command;
    let capabilities = &device.capabilities.0;
    if !capabilities.contains_key(&cluster) {
        let response = CommandResponse {
            success: false,
            message: format!(
                "Cluster '{}' not supported by device '{}'",
                cluster, device.name
            ),
        };
        return (StatusCode::BAD_REQUEST, Json(response));
    }
    // Check the command is supported by this cluster
    if capabilities[&cluster]
        .iter()
        .all(|cmd| cmd != &command)
    {
        let response = CommandResponse {
            success: false,
            message: format!(
                "Command '{}' not supported by device '{}' in cluster '{}'",
                command, device.name, cluster
            ),
        };
        return (StatusCode::BAD_REQUEST, Json(response));
    }

    // Build the command with arguments
    let mut cmd = Command::new("chip-tool");
    cmd.arg(&cluster)
        .arg(&command);
    for arg in payload.args {
        cmd.arg(arg);
    }
    cmd.arg(device.node_id.to_string())
        .arg(device.endpoint_id.to_string());
    
    // Execute the command using chip-tool
    let result = cmd.output();
    match result {
        Ok(output) => {
            // If the command executed successfully, return a success response
            if output.status.success() {
                let response = CommandResponse {
                    success: true,
                    message: format!(
                        "Command '{}' executed successfully on device '{}'",
                        command, device.name
                    ),
                };
                (StatusCode::OK, Json(response))
            // If the command failed, return an error response 
            // For now I treat all failures as a bad request
            } else {
                let response = CommandResponse {
                    success: false,
                    message: format!(
                        "Command execution failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ),
                };
                (StatusCode::BAD_REQUEST, Json(response))
            }
        }
        // If there was an error executing the command, return an error response
        Err(e) => {
            let response = CommandResponse {
                success: false,
                message: format!("Failed to execute command: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
    }
}

/// Handles device commissioning requests
///
/// This function processes incoming HTTP requests to commission new devices.
/// It should not be called directly; It is invoked by the Axum framework when a request
///
/// ### Parameters:
/// - State(state): State<AppState> - To access the database pool
/// - Json(payload): Json<CommisionRequest> - The JSON body contains the commissioning request
///
/// ### Returns:
/// - impl IntoResponse - An HTTP response indicating the result of the commissioning process
async fn handle_device_commission(
    State(state): State<AppState>,
    Json(payload): Json<CommissionRequest>,
) -> impl IntoResponse {
    // Check the payload is valid
    let pairing_code = payload.pairing_code;
    let name = payload.name;

    // Find the next available database ID
    let db_pool = &state.db_pool;
    let sql_result: Result<i32, sqlx::Error> =
        sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) + 1 FROM devices")
            .fetch_one(db_pool)
            .await;
    let node_id = match sql_result {
        Ok(id) => id,
        Err(e) => {
            let response = CommissionResponse {
                success: false,
                id: None,
                message: format!("Database error: {}", e),
            };
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(response));
        }
    };

    // Commission the device using chip-tool
    let result = Command::new("chip-tool")
        .arg("pairing")
        .arg("onnetwork")
        .arg(node_id.to_string())
        .arg(pairing_code.to_string())
        .output();
    // Handle command execution errors
    if result.is_err() {
        let response = CommissionResponse {
            success: false,
            id: None,
            message: format!(
                "Failed to execute commissioning command: {}",
                result.err().unwrap()
            ),
        };
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(response));
    }
    let result = result.unwrap();
    // Handle command failure
    if !result.status.success() {
        let response = CommissionResponse {
            success: false,
            id: None,
            message: format!(
                "Commissioning command failed: {}",
                String::from_utf8_lossy(&result.stderr)
            ),
        };
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(response));
    }

    // Ask the device about supported clusters / capabilities
    let result = Command::new("chip-tool")
        .arg("descriptor")
        .arg("read")
        .arg("server-list")
        .arg(node_id.to_string())
        .arg("1")
        .output()
        .expect("Failed to run command to get supported clusters");

    let stdout = String::from_utf8_lossy(&result.stdout);
    let re = Regex::new(r"\[TOO\].*?\[\d+\]:\s+(\d+)\s+\(").unwrap();
    let mut cluster_ids = Vec::new();

    // Extract cluster IDs using regex
    for line in stdout.lines() {
        for cap in re.captures_iter(line) {
            cluster_ids.push(cap[1].to_string());
        }
    }

    // Get names of supported clusters
    let supported_clusters: Vec<&str> = cluster_ids
        .iter()
        .filter_map(|cluster_id_str| {
            let cluster_id = u32::from_str_radix(cluster_id_str, 10).ok()?;
            get_cluster_name(cluster_id)
        })
        .collect();

    // For each supported cluster, get the supported commands
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for cluster in supported_clusters.iter() {
        let cluster_id = u32::from_str_radix(cluster, 10).unwrap();
        let cluster_name = get_cluster_name(cluster_id).unwrap_or("unknown");
        // Skip unknown clusters
        if cluster_name == "unknown" {
            continue;
        }
        // GRun the commands to find the supported commands for this cluster
        let result = Command::new("chip-tool")
            .arg(cluster_name)
            .arg("read")
            .arg("accepted-command-list")
            .arg(node_id.to_string())
            .arg("1")
            .output()
            .expect("Failed to execute command to get supported commands");
        let stdout = String::from_utf8_lossy(&result.stdout);
        let re = Regex::new(r"\[TOO\].*?\[\d+\]:\s+(\d+)\s+\(").unwrap();
        let mut ids = Vec::new();

        // Extract command IDs using regex
        for line in stdout.lines() {
            for cap in re.captures_iter(line) {
                ids.push(cap[1].to_string());
            }
        }

        // Map command IDs to names
        let supported_commands: Vec<&str> = ids
            .iter()
            .filter_map(|cmd_id_str| {
                let cmd_id = u32::from_str_radix(cmd_id_str, 10).ok()?;
                get_command_name(cluster_id, cmd_id)
            })
            .collect();
        
        // Insert into final JSON
        map.insert(
            cluster_name.to_string(),
            supported_commands.iter().map(|s| s.to_string()).collect(),
        );
    }
    
        // Insert the new device into the database
    let insert_result = sqlx::query(
        "INSERT INTO devices (node_id, endpoint_id, name, capabilities) VALUES (?, ?, ?, ?)",
    )
    .bind(node_id)
    .bind(1) // Assuming endpoint_id is 1 for now
    .bind(name)
    .bind(serde_json::to_string(&sqlx::types::Json(map)).unwrap()) // Serialize capabilities to JSON string
    .execute(db_pool)
    .await;
    // Handle the result of the insert operation
    return match insert_result {
        Ok(_) => {
            let response = CommissionResponse {
                success: true,
                id: Some(node_id),
                message: "Device commissioned successfully".to_string(),
            };
            (StatusCode::OK, Json(response))
        }
        Err(e) => {
            let response = CommissionResponse {
                success: false,
                id: None,
                message: format!("Failed to insert device into database: {}", e),
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(response))
        }
    };
}

// --- Other Functions ---

/// Gets the name of a cluster based on its ID
///
/// ### Parameters:
/// - cluster_id: u32 - The ID of the cluster
///
/// ### Returns:
/// - Option<&'static str> - The name of the cluster if found, otherwise None
///
/// ---
///
/// ### Example:
///
/// ```
/// let cluster_name = get_cluster_name(0x06);
/// assert_eq!(cluster_name, Some("onoff"));
/// ```
fn get_cluster_name(cluster_id: u32) -> Option<&'static str> {
    match cluster_id {
        0x06 => Some("onoff"),
        0x08 => Some("levelcontrol"),
        0x300 => Some("colorcontrol"),
        _ => None,
    }
}

/// Gets the name of a command based on its cluster ID and command ID
///
/// ### Parameters:
/// - cluster_id: u32 - The ID of the cluster
/// - command_id: u32 - The ID of the command
///
/// ### Returns:
/// - Option<&'static str> - The name of the command if found, otherwise None
///
/// ---
///
/// ### Example:
/// / ```
/// let command_name = get_command_name(0x06, 0x01);
/// assert_eq!(command_name, Some("on"));
/// ```
fn get_command_name(cluster_id: u32, command_id: u32) -> Option<&'static str> {
    match (cluster_id, command_id) {
        // 6 = On/Off
        (0x06, 0x00) => Some("off"),
        (0x06, 0x01) => Some("on"),
        (0x06, 0x02) => Some("toggle"),

        // 8 = 0x0Level Control
        (0x08, 0x00) => Some("move-to-level"),
        (0x08, 0x01) => Some("move"),
        (0x08, 0x02) => Some("step"),
        (0x08, 0x03) => Some("stop"),
        (0x08, 0x04) => Some("move-to-level-with-on-off"),
        (0x08, 0x05) => Some("move-with-on-off"),
        (0x08, 0x06) => Some("step-with-on-off"),
        (0x08, 0x07) => Some("stop-with-on-off"),
        (0x08, 0x08) => Some("move-to-closest-frequency"),

        // 768 = Color Control
        (0x300, 0x00) => Some("move-to-hue"),
        (0x300, 0x01) => Some("move-hue"),
        (0x300, 0x02) => Some("step-hue"),
        (0x300, 0x03) => Some("move-to-saturation"),
        (0x300, 0x04) => Some("move-saturation"),
        (0x300, 0x05) => Some("step-saturation"),
        (0x300, 0x06) => Some("move-to-hue-and-saturation"),
        (0x300, 0x07) => Some("move-to-color"),
        (0x300, 0x08) => Some("move-color"),
        (0x300, 0x09) => Some("step-color"),
        (0x300, 0x0A) => Some("move-to-color-temperature"),
        (0x300, 0x40) => Some("enhanced-move-to-hue"),
        (0x300, 0x41) => Some("enhanced-move-hue"),
        (0x300, 0x42) => Some("enhanced-step-hue"),
        (0x300, 0x43) => Some("enhanced-move-to-hue-and-saturation"),
        (0x300, 0x44) => Some("color-loop-set"),
        (0x300, 0x47) => Some("stop-move-set"),
        (0x300, 0x4B) => Some("move-color-temperature"),
        (0x300, 0x4C) => Some("step-color-temperature"),

        _ => None,
    }
}
