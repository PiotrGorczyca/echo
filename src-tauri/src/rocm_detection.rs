use std::process::Command;
use std::collections::HashSet;
use serde::{Deserialize, Serialize};

/// List of ROCm-supported GPUs based on AMD's official documentation
/// https://rocm.docs.amd.com/projects/install-on-linux/en/latest/reference/system-requirements.html#rdna-os
const SUPPORTED_ROCM_GPUS: &[&str] = &[
    // AMD Instinct (CDNA3, CDNA2, CDNA)
    "AMD Instinct MI325X",
    "AMD Instinct MI300X", 
    "AMD Instinct MI300A",
    "AMD Instinct MI250X",
    "AMD Instinct MI250",
    "AMD Instinct MI210",
    "AMD Instinct MI100",
    
    // AMD Radeon PRO (RDNA4, RDNA3, RDNA2)
    "AMD Radeon AI PRO R9700",
    "AMD Radeon PRO V710",
    "AMD Radeon PRO W7900 Dual Slot",
    "AMD Radeon PRO W7900",
    "AMD Radeon PRO W7800 48GB",
    "AMD Radeon PRO W7800",
    "AMD Radeon PRO W7700",
    "AMD Radeon PRO W6800",
    "AMD Radeon PRO V620",
    
    // AMD Radeon (RDNA4, RDNA3)
    "AMD Radeon RX 9070 XT",
    "AMD Radeon RX 9070 GRE", 
    "AMD Radeon RX 9070",
    "AMD Radeon RX 9060 XT",
    "AMD Radeon RX 7900 XTX",
    "AMD Radeon RX 7900 XT",
    "AMD Radeon RX 7900 GRE",
    "AMD Radeon RX 7800 XT",
];

/// Additional patterns to match GPU names that might have slight variations
const SUPPORTED_GPU_PATTERNS: &[&str] = &[
    "MI325X", "MI300X", "MI300A", "MI250X", "MI250", "MI210", "MI100",
    "R9700", "V710", "W7900", "W7800", "W7700", "W6800", "V620",
    "RX 9070", "RX 9060", "RX 7900", "RX 7800",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RocmCompatibility {
    pub is_supported: bool,
    pub detected_gpus: Vec<String>,
    pub supported_gpus: Vec<String>,
    pub reason: String,
}

/// Detect if the system has ROCm-compatible GPUs
pub fn detect_rocm_compatibility() -> RocmCompatibility {
    let detected_gpus = get_system_gpus();
    let supported_gpus = find_supported_gpus(&detected_gpus);
    
    let is_supported = !supported_gpus.is_empty();
    let reason = if is_supported {
        format!("Found {} ROCm-compatible GPU(s)", supported_gpus.len())
    } else if detected_gpus.is_empty() {
        "No AMD GPUs detected on system".to_string()
    } else {
        format!("Detected AMD GPU(s) but none are ROCm-compatible: {}", 
                detected_gpus.join(", "))
    };
    
    RocmCompatibility {
        is_supported,
        detected_gpus,
        supported_gpus,
        reason,
    }
}

/// Get list of AMD GPUs on the system
fn get_system_gpus() -> Vec<String> {
    let mut gpus = Vec::new();
    
    // Try multiple methods to detect GPUs
    
    // Method 1: rocm-smi (most reliable for ROCm)
    if let Ok(output) = Command::new("rocm-smi")
        .args(&["--showproductname"])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("Card Series:") {
                    if let Some(gpu_name) = line.split("Card Series:").nth(1) {
                        let gpu_name = gpu_name.trim();
                        if !gpu_name.is_empty() && gpu_name.contains("AMD") {
                            gpus.push(gpu_name.to_string());
                        }
                    }
                }
            }
        }
    }
    
    // Method 2: lspci (fallback)
    if gpus.is_empty() {
        if let Ok(output) = Command::new("lspci")
            .args(&["-nn"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("VGA") || line.contains("Display") {
                        if line.contains("AMD") || line.contains("ATI") {
                            // Extract GPU name from lspci output
                            if let Some(gpu_part) = line.split("]: ").nth(1) {
                                let gpu_name = gpu_part.split(" [").next().unwrap_or(gpu_part);
                                gpus.push(gpu_name.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Method 3: /proc/driver/nvidia/gpus check (to exclude NVIDIA)
    // This helps us avoid false positives when both AMD and NVIDIA are present
    
    // Remove duplicates
    let unique_gpus: HashSet<String> = gpus.into_iter().collect();
    unique_gpus.into_iter().collect()
}

/// Find which detected GPUs are supported by ROCm
fn find_supported_gpus(detected_gpus: &[String]) -> Vec<String> {
    let mut supported = Vec::new();
    
    for gpu in detected_gpus {
        // Check exact matches first
        if SUPPORTED_ROCM_GPUS.iter().any(|&supported_gpu| 
            gpu.to_lowercase().contains(&supported_gpu.to_lowercase())
        ) {
            supported.push(gpu.clone());
            continue;
        }
        
        // Check pattern matches
        if SUPPORTED_GPU_PATTERNS.iter().any(|&pattern| 
            gpu.to_uppercase().contains(&pattern.to_uppercase())
        ) {
            supported.push(gpu.clone());
        }
    }
    
    supported
}

/// Check if ROCm is installed and working
pub fn is_rocm_installed() -> bool {
    // Check if rocm-smi exists and works
    if let Ok(output) = Command::new("rocm-smi").arg("--version").output() {
        return output.status.success();
    }
    
    // Check if ROCm libraries exist
    std::path::Path::new("/opt/rocm").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_supported_gpu_detection() {
        let test_gpus = vec![
            "AMD Radeon RX 7900 XTX".to_string(),
            "AMD Radeon RX 7600S".to_string(),
            "AMD Instinct MI300X".to_string(),
        ];
        
        let supported = find_supported_gpus(&test_gpus);
        
        // Should find RX 7900 XTX and MI300X, but not RX 7600S
        assert_eq!(supported.len(), 2);
        assert!(supported.iter().any(|gpu| gpu.contains("7900 XTX")));
        assert!(supported.iter().any(|gpu| gpu.contains("MI300X")));
        assert!(!supported.iter().any(|gpu| gpu.contains("7600S")));
    }
} 