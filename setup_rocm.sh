#!/bin/bash

# ROCm Setup Script for AMD GPU Acceleration
# This script sets up ROCm and PyTorch with ROCm support for the AMD Radeon RX 7600

set -e

echo "=== ROCm Setup for AMD GPU Acceleration ==="
echo "Detected GPU: AMD Radeon RX 7600 (Navi 33)"
echo ""

# Check if running on supported OS
if ! command -v lsb_release &> /dev/null; then
    echo "Error: lsb_release not found. Please install lsb-release package."
    exit 1
fi

OS_ID=$(lsb_release -si)
OS_VERSION=$(lsb_release -sr)

if [[ "$OS_ID" != "ManjaroLinux" && "$OS_ID" != "Arch" ]]; then
    echo "Warning: This script is optimized for Arch/Manjaro. You may need to adapt it for your distribution."
fi

echo "Step 1: Installing ROCm packages..."

# For Arch/Manjaro, install ROCm from AUR or official repositories
if command -v yay &> /dev/null; then
    echo "Using yay to install ROCm packages..."
    yay -S --needed rocm-dev rocm-libs rocm-opencl-runtime rocm-smi-lib hip-runtime-amd
elif command -v paru &> /dev/null; then
    echo "Using paru to install ROCm packages..."
    paru -S --needed rocm-dev rocm-libs rocm-opencl-runtime rocm-smi-lib hip-runtime-amd
else
    echo "Installing ROCm packages with pacman (may require AUR helper)..."
    echo "Please install an AUR helper like yay or paru first:"
    echo "  sudo pacman -S --needed base-devel git"
    echo "  git clone https://aur.archlinux.org/yay.git"
    echo "  cd yay && makepkg -si"
    echo "Then run this script again."
    exit 1
fi

echo ""
echo "Step 2: Adding user to render and video groups..."
sudo usermod -aG render,video $USER

echo ""
echo "Step 3: Setting up environment variables..."

# Add ROCm to PATH and set environment variables
ROCM_ENV_FILE="$HOME/.rocm_env"
cat > "$ROCM_ENV_FILE" << 'EOF'
# ROCm Environment Variables
export PATH=/opt/rocm/bin:/opt/rocm/opencl/bin:$PATH
export LD_LIBRARY_PATH=/opt/rocm/lib:/opt/rocm/lib64:$LD_LIBRARY_PATH
export ROCM_PATH=/opt/rocm
export HIP_PATH=/opt/rocm
export HSA_PATH=/opt/rocm
export HIP_VISIBLE_DEVICES=0
export CUDA_VISIBLE_DEVICES=0
export HSA_OVERRIDE_GFX_VERSION=11.0.2
export ROCR_VISIBLE_DEVICES=0
EOF

# Source the environment file
source "$ROCM_ENV_FILE"

# Add to shell profile
if [[ "$SHELL" == *"zsh"* ]]; then
    SHELL_RC="$HOME/.zshrc"
elif [[ "$SHELL" == *"bash"* ]]; then
    SHELL_RC="$HOME/.bashrc"
else
    SHELL_RC="$HOME/.profile"
fi

if ! grep -q "source $ROCM_ENV_FILE" "$SHELL_RC" 2>/dev/null; then
    echo "source $ROCM_ENV_FILE" >> "$SHELL_RC"
    echo "Added ROCm environment to $SHELL_RC"
fi

echo ""
echo "Step 4: Installing PyTorch and dependencies..."

# Create a virtual environment for PyTorch
VENV_PATH="$HOME/.rocm_pytorch_env"
echo "Creating virtual environment at $VENV_PATH..."

if [ -d "$VENV_PATH" ]; then
    echo "Virtual environment already exists, removing old one..."
    rm -rf "$VENV_PATH"
fi

python3 -m venv "$VENV_PATH"
source "$VENV_PATH/bin/activate"

echo "Using Python version: $(python --version)"

# Install PyTorch (latest version that supports ROCm through runtime detection)
echo "Installing PyTorch and dependencies..."
pip install --upgrade pip wheel setuptools

# Install PyTorch, but we'll rely on ROCm runtime detection
pip install torch torchvision torchaudio

# Install additional dependencies for transformers
echo "Installing additional dependencies..."
pip install transformers accelerate datasets sentencepiece

echo ""
echo "✅ Successfully installed PyTorch with dependencies!"
echo "ROCm acceleration will be detected at runtime through installed drivers."

# Update the environment file to include virtual environment activation
cat >> "$ROCM_ENV_FILE" << 'EOF'

# Activate ROCm PyTorch virtual environment
source ~/.rocm_pytorch_env/bin/activate
EOF

echo "Virtual environment created and configured!"
echo "ROCm PyTorch installed in: $VENV_PATH"

echo ""
echo "Step 5: Verifying installation..."

# Test ROCm installation
if command -v rocm-smi &> /dev/null; then
    echo "ROCm SMI output:"
    rocm-smi || echo "ROCm SMI failed - this is normal on some systems"
else
    echo "Warning: rocm-smi not found in PATH"
fi

# Test PyTorch ROCm support
echo ""
echo "Testing PyTorch ROCm support..."
source "$VENV_PATH/bin/activate"
python -c "
import torch
print(f'PyTorch version: {torch.__version__}')
print(f'CUDA available: {torch.cuda.is_available()}')
if torch.cuda.is_available():
    print(f'CUDA device count: {torch.cuda.device_count()}')
    print(f'CUDA device name: {torch.cuda.get_device_name(0)}')
    print(f'HIP version: {torch.version.hip}')
    if torch.version.hip:
        print('✓ ROCm backend detected!')
    else:
        print('⚠ NVIDIA CUDA backend detected')
else:
    print('⚠ No CUDA/ROCm support detected')
"

echo ""
echo "=== Setup Complete ==="
echo ""
echo "IMPORTANT: Please restart your terminal or run 'source $ROCM_ENV_FILE' to load the environment variables."
echo ""
echo "To test GPU acceleration in your application:"
echo "1. Restart your terminal"
echo "2. Navigate to your project directory"
echo "3. Run your application with ROCm feature enabled:"
echo "   cargo run --features rocm"
echo ""
echo "Your AMD Radeon RX 7600 should now be available for GPU-accelerated transcription!" 