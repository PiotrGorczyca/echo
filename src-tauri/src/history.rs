use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionHistoryEntry {
    pub id: String,
    pub text: String,
    pub timestamp: DateTime<Utc>,
    pub source: TranscriptionSource,
    pub duration_ms: Option<u64>,
    pub model: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionSource {
    Manual,
    VoiceCommand,
    Meeting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionHistory {
    entries: Vec<TranscriptionHistoryEntry>,
    max_entries: usize,
}

impl Default for TranscriptionHistory {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 1000, // Keep last 1000 entries
        }
    }
}

impl TranscriptionHistory {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    pub fn add_entry(&mut self, text: String, source: TranscriptionSource, duration_ms: Option<u64>, model: Option<String>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let entry = TranscriptionHistoryEntry {
            id: id.clone(),
            text,
            timestamp: Utc::now(),
            source,
            duration_ms,
            model,
            pinned: false,
        };

        self.entries.insert(0, entry); // Insert at beginning for reverse chronological order

        // Trim to max entries
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }

        id
    }

    pub fn get_entries(&self) -> Vec<TranscriptionHistoryEntry> {
        // Return sorted entries: Pinned first (by timestamp descending), then Unpinned (by timestamp descending)
        // Since entries are already stored in reverse chronological order (mostly), we just need to stable sort by pinned status.
        
        let mut sorted_entries = self.entries.clone();
        sorted_entries.sort_by(|a, b| {
            // First compare pinned status (true > false)
            // Then compare timestamp (newer > older)
            
            if a.pinned != b.pinned {
                // If a is pinned (true) and b is not (false), a should come first
                b.pinned.cmp(&a.pinned)
            } else {
                // Both pinned or both unpinned, sort by timestamp descending
                b.timestamp.cmp(&a.timestamp)
            }
        });
        
        sorted_entries
    }

    pub fn get_entry(&self, id: &str) -> Option<TranscriptionHistoryEntry> {
        self.entries.iter().find(|e| e.id == id).cloned()
    }

    pub fn delete_entry(&mut self, id: &str) -> bool {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            self.entries.remove(pos);
            true
        } else {
            false
        }
    }

    pub fn toggle_pin(&mut self, id: &str) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
            entry.pinned = !entry.pinned;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn search(&self, query: &str) -> Vec<TranscriptionHistoryEntry> {
        let query_lower = query.to_lowercase();
        let mut matches: Vec<TranscriptionHistoryEntry> = self.entries
            .iter()
            .filter(|e| e.text.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();

        // Sort matches same as get_entries
        matches.sort_by(|a, b| {
            if a.pinned != b.pinned {
                b.pinned.cmp(&a.pinned)
            } else {
                b.timestamp.cmp(&a.timestamp)
            }
        });
        
        matches
    }
}

pub struct HistoryManager {
    history: Arc<Mutex<TranscriptionHistory>>,
    storage_path: PathBuf,
}

impl HistoryManager {
    pub fn new(storage_path: PathBuf) -> Result<Self, std::io::Error> {
        // Ensure parent directory exists
        if let Some(parent) = storage_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Load existing history or create new
        let history = if storage_path.exists() {
            match fs::read_to_string(&storage_path) {
                Ok(content) => {
                    serde_json::from_str(&content).unwrap_or_default()
                }
                Err(_) => TranscriptionHistory::default(),
            }
        } else {
            TranscriptionHistory::default()
        };

        Ok(Self {
            history: Arc::new(Mutex::new(history)),
            storage_path,
        })
    }

    pub fn add_transcription(
        &self,
        text: String,
        source: TranscriptionSource,
        duration_ms: Option<u64>,
        model: Option<String>,
    ) -> Result<String, String> {
        let id = {
            let mut history = self.history.lock().map_err(|e| e.to_string())?;
            history.add_entry(text, source, duration_ms, model)
        };

        self.save()?;
        Ok(id)
    }

    pub fn get_history(&self) -> Result<Vec<TranscriptionHistoryEntry>, String> {
        let history = self.history.lock().map_err(|e| e.to_string())?;
        Ok(history.get_entries())
    }

    pub fn get_entry(&self, id: &str) -> Result<Option<TranscriptionHistoryEntry>, String> {
        let history = self.history.lock().map_err(|e| e.to_string())?;
        Ok(history.get_entry(id))
    }

    pub fn delete_entry(&self, id: &str) -> Result<bool, String> {
        let result = {
            let mut history = self.history.lock().map_err(|e| e.to_string())?;
            history.delete_entry(id)
        };

        if result {
            self.save()?;
        }

        Ok(result)
    }

    pub fn toggle_pin(&self, id: &str) -> Result<bool, String> {
        let result = {
            let mut history = self.history.lock().map_err(|e| e.to_string())?;
            history.toggle_pin(id)
        };

        if result {
            self.save()?;
        }

        Ok(result)
    }

    pub fn clear_history(&self) -> Result<(), String> {
        {
            let mut history = self.history.lock().map_err(|e| e.to_string())?;
            history.clear();
        }

        self.save()?;
        Ok(())
    }

    pub fn search(&self, query: &str) -> Result<Vec<TranscriptionHistoryEntry>, String> {
        let history = self.history.lock().map_err(|e| e.to_string())?;
        Ok(history.search(query))
    }

    fn save(&self) -> Result<(), String> {
        let history = self.history.lock().map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&*history)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        fs::write(&self.storage_path, json)
            .map_err(|e| format!("Failed to write history file: {}", e))?;

        Ok(())
    }
}
