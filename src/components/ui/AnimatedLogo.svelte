<script lang="ts">
  export let isRecording = false;
  export let state: 'idle' | 'recording' | 'transcribing' | 'processing' | 'executing' = 'idle';
  export let size = '120px';
  
  // Logo path from static assets
  const logoSrc = '/favicon.png';
  
  // Get animation class based on state
  function getAnimationClass(currentState: string): string {
    switch (currentState) {
      case 'recording':
        return 'pulse-recording';
      case 'transcribing':
        return 'spin-transcribing';
      case 'processing':
        return 'pulse-processing';
      case 'executing':
        return 'glow-executing';
      default:
        return 'idle';
    }
  }
  
  // Get accent color based on state
  function getAccentColor(currentState: string): string {
    switch (currentState) {
      case 'recording':
        return '#FF4444'; // Red for recording
      case 'transcribing':
        return '#4A90E2'; // Blue for transcribing
      case 'processing':
        return '#9C27B0'; // Purple for processing
      case 'executing':
        return '#4CAF50'; // Green for executing
      default:
        return '#666666'; // Gray for idle
    }
  }
</script>

<div class="animated-logo-container" style="--size: {size}; --accent-color: {getAccentColor(state)}">
  <div class="logo-wrapper {getAnimationClass(state)}">
    <!-- Outer ring that rotates -->
    <div class="outer-ring"></div>
    
    <!-- Middle ring that pulses -->
    <div class="middle-ring"></div>
    
    <!-- Inner glow effect -->
    <div class="inner-glow"></div>
    
    <!-- The actual logo -->
    <img src={logoSrc} alt="EchoType" class="logo-image" />
    
    <!-- Recording indicator dots -->
    {#if isRecording}
      <div class="recording-dots">
        <div class="dot dot-1"></div>
        <div class="dot dot-2"></div>
        <div class="dot dot-3"></div>
      </div>
    {/if}
  </div>
  
  <!-- State indicator text -->
  <div class="state-text">
    {#if state === 'recording'}
      🎤 Recording...
    {:else if state === 'transcribing'}
      📝 Transcribing...
    {:else if state === 'processing'}
      🧠 Processing...
    {:else if state === 'executing'}
      ⚡ Executing...
    {:else}
      💬 Ready
    {/if}
  </div>
</div>

<style>
  .animated-logo-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    user-select: none;
  }

  .logo-wrapper {
    position: relative;
    width: var(--size);
    height: var(--size);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Base ring styles */
  .outer-ring,
  .middle-ring,
  .inner-glow {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    pointer-events: none;
  }

  .outer-ring {
    border: 2px solid var(--accent-color);
    opacity: 0.6;
  }

  .middle-ring {
    border: 1px solid var(--accent-color);
    opacity: 0.4;
    transform: scale(0.8);
  }

  .inner-glow {
    background: radial-gradient(circle, var(--accent-color) 0%, transparent 70%);
    opacity: 0.1;
    transform: scale(0.6);
  }

  .logo-image {
    width: 60%;
    height: 60%;
    object-fit: contain;
    position: relative;
    z-index: 10;
    filter: drop-shadow(0 0 8px rgba(0, 0, 0, 0.3));
  }

  .recording-dots {
    position: absolute;
    bottom: -8px;
    right: -8px;
    display: flex;
    gap: 4px;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #FF4444;
    opacity: 0.8;
  }

  .state-text {
    font-size: 0.9rem;
    font-weight: 500;
    color: var(--accent-color);
    text-align: center;
    min-height: 1.2em;
    transition: color 0.3s ease;
  }

  /* Animation classes */
  .idle {
    transition: all 0.3s ease;
  }

  .idle .outer-ring,
  .idle .middle-ring,
  .idle .inner-glow {
    transform: scale(1);
    opacity: 0.3;
  }

  /* Recording animation - pulsing red */
  .pulse-recording {
    animation: pulseRecording 1.5s ease-in-out infinite;
  }

  .pulse-recording .outer-ring {
    animation: ringPulse 1.5s ease-in-out infinite;
  }

  .pulse-recording .middle-ring {
    animation: ringPulse 1.5s ease-in-out infinite 0.2s;
  }

  .pulse-recording .inner-glow {
    animation: glowPulse 1.5s ease-in-out infinite 0.4s;
  }

  .pulse-recording .logo-image {
    animation: imagePulse 1.5s ease-in-out infinite;
  }

  /* Recording dots animation */
  .dot-1 {
    animation: dotBlink 1s ease-in-out infinite 0s;
  }

  .dot-2 {
    animation: dotBlink 1s ease-in-out infinite 0.2s;
  }

  .dot-3 {
    animation: dotBlink 1s ease-in-out infinite 0.4s;
  }

  /* Transcribing animation - spinning blue */
  .spin-transcribing {
    animation: spinTranscribing 2s linear infinite;
  }

  .spin-transcribing .outer-ring {
    animation: spinRing 3s linear infinite;
  }

  .spin-transcribing .middle-ring {
    animation: spinRing 2s linear infinite reverse;
  }

  .spin-transcribing .inner-glow {
    animation: glowPulse 1.5s ease-in-out infinite;
  }

  /* Processing animation - purple glow pulse */
  .pulse-processing {
    animation: pulseProcessing 2s ease-in-out infinite;
  }

  .pulse-processing .outer-ring {
    animation: processingRing 2s ease-in-out infinite;
  }

  .pulse-processing .middle-ring {
    animation: processingRing 2s ease-in-out infinite 0.5s;
  }

  .pulse-processing .inner-glow {
    animation: processingGlow 2s ease-in-out infinite;
  }

  /* Executing animation - green success glow */
  .glow-executing {
    animation: executeGlow 1s ease-in-out infinite;
  }

  .glow-executing .outer-ring {
    animation: executeRing 1s ease-in-out infinite;
  }

  .glow-executing .middle-ring {
    animation: executeRing 1s ease-in-out infinite 0.3s;
  }

  .glow-executing .inner-glow {
    animation: executeInnerGlow 1s ease-in-out infinite;
  }

  /* Keyframes */
  @keyframes pulseRecording {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.05); }
  }

  @keyframes ringPulse {
    0%, 100% { 
      transform: scale(1); 
      opacity: 0.6; 
    }
    50% { 
      transform: scale(1.1); 
      opacity: 0.9; 
    }
  }

  @keyframes glowPulse {
    0%, 100% { 
      opacity: 0.1; 
      transform: scale(0.6);
    }
    50% { 
      opacity: 0.3; 
      transform: scale(0.8);
    }
  }

  @keyframes imagePulse {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.1); }
  }

  @keyframes dotBlink {
    0%, 100% { opacity: 0.3; }
    50% { opacity: 1; }
  }

  @keyframes spinTranscribing {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  @keyframes spinRing {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  @keyframes pulseProcessing {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.08); }
  }

  @keyframes processingRing {
    0%, 100% { 
      transform: scale(1); 
      opacity: 0.4;
    }
    50% { 
      transform: scale(1.15); 
      opacity: 0.8;
    }
  }

  @keyframes processingGlow {
    0%, 100% { 
      opacity: 0.1; 
      transform: scale(0.6);
    }
    50% { 
      opacity: 0.4; 
      transform: scale(0.9);
    }
  }

  @keyframes executeGlow {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.1); }
  }

  @keyframes executeRing {
    0%, 100% { 
      transform: scale(1); 
      opacity: 0.6;
    }
    50% { 
      transform: scale(1.2); 
      opacity: 1;
    }
  }

  @keyframes executeInnerGlow {
    0%, 100% { 
      opacity: 0.2; 
      transform: scale(0.6);
    }
    50% { 
      opacity: 0.6; 
      transform: scale(1);
    }
  }

  /* Responsive adjustments */
  @media (max-width: 768px) {
    .animated-logo-container {
      gap: 12px;
    }
    
    .state-text {
      font-size: 0.8rem;
    }
    
    .recording-dots {
      bottom: -6px;
      right: -6px;
    }
    
    .dot {
      width: 6px;
      height: 6px;
    }
  }

  /* Reduced motion support */
  @media (prefers-reduced-motion: reduce) {
    .pulse-recording,
    .spin-transcribing,
    .pulse-processing,
    .glow-executing {
      animation: none;
    }
    
    .pulse-recording .outer-ring,
    .pulse-recording .middle-ring,
    .pulse-recording .inner-glow,
    .pulse-recording .logo-image,
    .spin-transcribing .outer-ring,
    .spin-transcribing .middle-ring,
    .spin-transcribing .inner-glow,
    .pulse-processing .outer-ring,
    .pulse-processing .middle-ring,
    .pulse-processing .inner-glow,
    .glow-executing .outer-ring,
    .glow-executing .middle-ring,
    .glow-executing .inner-glow {
      animation: none;
    }
    
    .dot {
      animation: none;
      opacity: 1;
    }
  }
</style> 