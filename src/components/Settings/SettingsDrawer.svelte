<script lang="ts">
  import { createEventDispatcher, onMount } from 'svelte';

  export let isOpen = true;

  const dispatch = createEventDispatcher();

  function closeDrawer() {
    dispatch('close');
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) {
      closeDrawer();
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && isOpen) {
      closeDrawer();
    }
  }

  onMount(() => {
    if (isOpen) {
      document.addEventListener('keydown', handleKeydown);
      // Prevent body scroll when drawer is open
      document.body.style.overflow = 'hidden';
    }

    return () => {
      document.removeEventListener('keydown', handleKeydown);
      document.body.style.overflow = '';
    };
  });
</script>

{#if isOpen}
  <!-- Backdrop -->
  <div 
    class="drawer-backdrop" 
    onclick={handleBackdropClick}
    role="button"
    tabindex="-1"
    aria-label="Close settings"
  >
    <!-- Drawer Content -->
    <div 
      class="drawer-content"
      role="dialog"
      aria-modal="true"
      aria-labelledby="drawer-title"
    >
      <slot></slot>
    </div>
  </div>
{/if}

<style>
  .drawer-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    width: 100vw;
    height: 100vh;
    background-color: rgba(0, 0, 0, 0.5);
    z-index: 1000;
    animation: fadeIn 200ms ease-out;
    display: flex;
    justify-content: flex-end;
    align-items: stretch;
  }

  .drawer-content {
    width: 420px;
    height: 100vh;
    background-color: var(--bg-primary, #1a1a1a);
    border-left: 1px solid var(--border-primary, #404040);
    animation: slideInRight 300ms ease-out;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: -4px 0 20px rgba(0, 0, 0, 0.3);
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes slideInRight {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  /* Mobile responsiveness */
  @media (max-width: 768px) {
    .drawer-content {
      width: 100vw;
      animation: slideInRight 250ms ease-out;
    }
  }

  @media (max-width: 480px) {
    .drawer-backdrop {
      background-color: var(--bg-primary, #1a1a1a);
    }
    
    .drawer-content {
      border-left: none;
      box-shadow: none;
    }
  }

  /* Focus management */
  .drawer-content:focus-within {
    outline: none;
  }

  /* Reduced motion support */
  @media (prefers-reduced-motion: reduce) {
    .drawer-backdrop {
      animation: none;
    }
    
    .drawer-content {
      animation: none;
    }
  }
</style> 