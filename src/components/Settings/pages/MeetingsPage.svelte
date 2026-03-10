<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  // Types based on the Rust structs
  interface Meeting {
    id: string;
    title: string;
    start_time: string;
    end_time?: string;
    duration?: { secs: number; nanos: number };
    participants: string[];
    audio_chunks: AudioChunk[];
    transcript?: string;
    action_items: ActionItem[];
    status: MeetingStatus;
    audio_directory: string;
  }

  interface AudioChunk {
    id: string;
    chunk_number: number;
    file_path: string;
    start_timestamp: string;
    end_timestamp?: string;
    duration_seconds: number;
    file_size_bytes: number;
  }

  interface ActionItem {
    id: string;
    meeting_id: string;
    text: string;
    assignee?: string;
    due_date?: string;
    priority: 'Low' | 'Medium' | 'High' | 'Critical';
    category: 'Task' | 'Decision' | 'FollowUp' | 'Question' | 'Note';
    context: string;
    status: 'Pending' | 'InProgress' | 'Completed' | 'Cancelled';
    timestamp_in_meeting?: number;
  }

  interface MeetingSummary {
    id: string;
    title: string;
    start_time: string;
    end_time?: string;
    duration?: { secs: number; nanos: number };
    participants: string[];
    status: MeetingStatus;
    action_item_count: number;
    has_transcript: boolean;
  }

  type MeetingStatus = 'Scheduled' | 'InProgress' | 'Recording' | 'Paused' | 'Processing' | 'Completed' | { Failed: string };

  interface MeetingRecordingState {
    is_recording: boolean;
    is_paused: boolean;
    current_chunk_number: number;
    current_chunk_path?: string;
    current_chunk_start_time?: string;
    total_recording_duration: { secs: number; nanos: number };
    last_save_time?: string;
  }

  interface ProcessingStatus {
    meeting_id: string;
    status: 'Pending' | 'Transcribing' | 'ExtractingActionItems' | 'Finalizing' | 'Completed' | 'Failed';
    progress: number;
    current_step: string;
    estimated_completion?: string;
    error_message?: string;
  }

  // State
  let meetings: MeetingSummary[] = [];
  let currentMeeting: Meeting | null = null;
  let recordingState: MeetingRecordingState | null = null;
  let selectedMeeting: Meeting | null = null;
  let processingStatuses: Record<string, ProcessingStatus> = {};
  let viewMode: 'list' | 'detail' | 'new' = 'list';
  let loading = false;
  let error = '';

  // New meeting form
  let newMeetingTitle = '';
  let newMeetingParticipants = '';

  // Listeners
  let eventListeners: Array<() => void> = [];

  onMount(async () => {
    await loadMeetings();
    await loadCurrentMeeting();
    await loadRecordingState();
    await loadProcessingStatuses();
    setupEventListeners();
  });

  onDestroy(() => {
    eventListeners.forEach(unlisten => unlisten());
  });

  async function loadMeetings() {
    try {
      loading = true;
      error = '';
      meetings = await invoke('list_meetings');
    } catch (err) {
      error = `Failed to load meetings: ${err}`;
      console.error('Load meetings error:', err);
    } finally {
      loading = false;
    }
  }

  async function loadCurrentMeeting() {
    try {
      currentMeeting = await invoke('get_current_meeting');
    } catch (err) {
      console.error('Load current meeting error:', err);
    }
  }

  async function loadRecordingState() {
    try {
      recordingState = await invoke('get_meeting_recording_state');
    } catch (err) {
      console.error('Load recording state error:', err);
    }
  }

  async function loadProcessingStatuses() {
    try {
      processingStatuses = await invoke('get_all_processing_statuses');
    } catch (err) {
      console.error('Load processing statuses error:', err);
    }
  }

  async function setupEventListeners() {
    try {
      // Meeting events
      eventListeners.push(await listen('meeting-started', () => {
        loadMeetings();
        loadCurrentMeeting();
      }));

      eventListeners.push(await listen('meeting-ended', () => {
        loadMeetings();
        loadCurrentMeeting();
        loadRecordingState();
      }));

      eventListeners.push(await listen('meeting-deleted', () => {
        loadMeetings();
        if (selectedMeeting && meetings.findIndex(m => m.id === selectedMeeting!.id) === -1) {
          selectedMeeting = null;
          viewMode = 'list';
        }
      }));

      // Recording events
      eventListeners.push(await listen('meeting-recording-started', () => {
        loadRecordingState();
      }));

      eventListeners.push(await listen('meeting-recording-paused', () => {
        loadRecordingState();
      }));

      eventListeners.push(await listen('meeting-recording-resumed', () => {
        loadRecordingState();
      }));

      eventListeners.push(await listen('meeting-recording-stopped', () => {
        loadRecordingState();
      }));

    } catch (err) {
      console.error('Failed to setup event listeners:', err);
    }
  }

  async function startNewMeeting() {
    try {
      loading = true;
      error = '';

      const participants = newMeetingParticipants
        .split(',')
        .map(p => p.trim())
        .filter(p => p.length > 0);

      if (!newMeetingTitle.trim()) {
        error = 'Meeting title is required';
        return;
      }

      const meetingId = await invoke('start_meeting', {
        title: newMeetingTitle.trim(),
        participants
      });

      // Start recording immediately
      await invoke('start_meeting_recording');

      // Reset form
      newMeetingTitle = '';
      newMeetingParticipants = '';
      viewMode = 'list';

      await loadMeetings();
      await loadCurrentMeeting();
      await loadRecordingState();

    } catch (err) {
      error = `Failed to start meeting: ${err}`;
      console.error('Start meeting error:', err);
    } finally {
      loading = false;
    }
  }

  async function pauseRecording() {
    try {
      await invoke('pause_meeting_recording');
    } catch (err) {
      error = `Failed to pause recording: ${err}`;
    }
  }

  async function resumeRecording() {
    try {
      await invoke('resume_meeting_recording');
    } catch (err) {
      error = `Failed to resume recording: ${err}`;
    }
  }

  // stopRecording function removed - recording is now stopped by endMeeting

  async function endMeeting() {
    try {
      loading = true;
      await invoke('end_meeting');
      await loadMeetings();
      await loadCurrentMeeting();
      await loadRecordingState();
    } catch (err) {
      error = `Failed to end meeting: ${err}`;
    } finally {
      loading = false;
    }
  }

  async function viewMeetingDetails(meetingId: string) {
    try {
      loading = true;
      selectedMeeting = await invoke('get_meeting', { meetingId });
      viewMode = 'detail';
    } catch (err) {
      error = `Failed to load meeting details: ${err}`;
    } finally {
      loading = false;
    }
  }

  async function deleteMeeting(meetingId: string) {
    if (confirm('Are you sure you want to delete this meeting? This action cannot be undone.')) {
      try {
        loading = true;
        await invoke('delete_meeting', { meetingId });
      } catch (err) {
        error = `Failed to delete meeting: ${err}`;
      } finally {
        loading = false;
      }
    }
  }

  function formatDuration(duration?: { secs: number; nanos: number }): string {
    if (!duration) return 'N/A';
    const totalSeconds = duration.secs;
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    
    if (hours > 0) {
      return `${hours}h ${minutes}m ${seconds}s`;
    } else if (minutes > 0) {
      return `${minutes}m ${seconds}s`;
    } else {
      return `${seconds}s`;
    }
  }

  function formatDate(dateStr: string): string {
    return new Date(dateStr).toLocaleString();
  }

  function getStatusColor(status: MeetingStatus): string {
    if (typeof status === 'object' && 'Failed' in status) return 'status-error';
    
    switch (status) {
      case 'Recording': return 'status-error';
      case 'Processing': return 'status-warning';
      case 'Completed': return 'status-success';
      case 'Paused': return 'status-warning';
      default: return 'status-info';
    }
  }

  function getStatusText(status: MeetingStatus): string {
    if (typeof status === 'object' && 'Failed' in status) {
      return `Failed: ${status.Failed}`;
    }
    return status;
  }

  function getPriorityColor(priority: string): string {
    switch (priority) {
      case 'Critical': return 'status-error';
      case 'High': return 'status-warning';
      case 'Medium': return 'status-info';
      case 'Low': return 'status-success';
      default: return '';
    }
  }
</script>

<div class="meetings-page">
  <div class="page-header">
    <div class="header-content">
      <h1>Meeting Transcription</h1>
      <div class="header-actions">
        {#if viewMode === 'list'}
          <button class="btn btn-primary" onclick={() => viewMode = 'new'}>
            📹 New Meeting
          </button>
        {:else if viewMode === 'detail'}
          <button class="btn" onclick={() => { viewMode = 'list'; selectedMeeting = null; }}>
            ← Back to List
          </button>
        {:else if viewMode === 'new'}
          <button class="btn" onclick={() => viewMode = 'list'}>
            ← Cancel
          </button>
        {/if}
      </div>
    </div>

    <!-- Current Meeting Status -->
    {#if currentMeeting && recordingState}
      <div class="current-meeting-status card">
        <div class="meeting-info">
          <h3>📹 {currentMeeting.title}</h3>
          <div class="meeting-details">
            <span>Started: {formatDate(currentMeeting.start_time)}</span>
            <span>•</span>
            <span>Participants: {currentMeeting.participants.join(', ') || 'None'}</span>
            <span>•</span>
            <span class={getStatusColor(currentMeeting.status)}>
              {getStatusText(currentMeeting.status)}
            </span>
          </div>
        </div>
        
        <div class="recording-controls">
          {#if recordingState.is_recording && !recordingState.is_paused}
            <button class="btn btn-warning" onclick={pauseRecording}>
              ⏸️ Pause Recording
            </button>
            <span class="recording-status">
              🔴 Recording in progress...
            </span>
          {:else if recordingState.is_recording && recordingState.is_paused}
            <button class="btn btn-primary" onclick={resumeRecording}>
              ▶️ Resume Recording
            </button>
            <span class="recording-status">
              ⏸️ Recording paused
            </span>
          {:else}
            <button class="btn btn-primary" onclick={() => invoke('start_meeting_recording')}>
              ⏺️ Start Recording
            </button>
          {/if}
          
          <button class="btn btn-error" onclick={endMeeting} disabled={loading}>
            {#if recordingState.is_recording}
              🏁 End Meeting & Stop Recording
            {:else}
              🏁 End Meeting
            {/if}
          </button>
        </div>
      </div>
    {/if}
  </div>

  <!-- Error Display -->
  {#if error}
    <div class="error-message card">
      <strong>Error:</strong> {error}
      <button class="btn btn-sm" onclick={() => error = ''}>✕</button>
    </div>
  {/if}

  <!-- Loading Indicator -->
  {#if loading}
    <div class="loading-indicator">
      <div class="spinner"></div>
      Loading...
    </div>
  {/if}

  <!-- Content Views -->
  <div class="page-content">
    {#if viewMode === 'new'}
      <!-- New Meeting Form -->
      <div class="new-meeting-form card">
        <h2>Start New Meeting</h2>
        
        <div class="form-group">
          <label for="meeting-title">Meeting Title</label>
          <input
            id="meeting-title"
            type="text"
            bind:value={newMeetingTitle}
            placeholder="Enter meeting title..."
            class="form-input"
          />
        </div>

        <div class="form-group">
          <label for="meeting-participants">Participants (comma-separated)</label>
          <input
            id="meeting-participants"
            type="text"
            bind:value={newMeetingParticipants}
            placeholder="John Doe, Jane Smith, etc."
            class="form-input"
          />
        </div>

        <div class="form-actions">
          <button class="btn btn-primary" onclick={startNewMeeting} disabled={loading || !newMeetingTitle.trim()}>
            🎬 Start Meeting & Recording
          </button>
          <button class="btn" onclick={() => viewMode = 'list'}>
            Cancel
          </button>
        </div>
      </div>

    {:else if viewMode === 'detail' && selectedMeeting}
      <!-- Meeting Detail View -->
      <div class="meeting-detail">
        <div class="meeting-header card">
          <div class="meeting-title">
            <h2>{selectedMeeting.title}</h2>
            <span class="meeting-id">ID: {selectedMeeting.id}</span>
          </div>
          
          <div class="meeting-meta">
            <div class="meta-item">
              <strong>Status:</strong>
              <span class={getStatusColor(selectedMeeting.status)}>
                {getStatusText(selectedMeeting.status)}
              </span>
            </div>
            <div class="meta-item">
              <strong>Started:</strong> {formatDate(selectedMeeting.start_time)}
            </div>
            {#if selectedMeeting.end_time}
              <div class="meta-item">
                <strong>Ended:</strong> {formatDate(selectedMeeting.end_time)}
              </div>
            {/if}
            <div class="meta-item">
              <strong>Duration:</strong> {formatDuration(selectedMeeting.duration)}
            </div>
            <div class="meta-item">
              <strong>Participants:</strong> {selectedMeeting.participants.join(', ') || 'None'}
            </div>
            <div class="meta-item">
              <strong>Audio Chunks:</strong> {selectedMeeting.audio_chunks.length}
            </div>
          </div>

          <div class="detail-actions">
            <button class="btn btn-error" onclick={() => deleteMeeting(selectedMeeting!.id)}>
              🗑️ Delete
            </button>
          </div>
        </div>

        <!-- Transcript Section -->
        {#if selectedMeeting.transcript}
          <div class="transcript-section card">
            <h3>📝 Transcript</h3>
            <div class="transcript-content">
              {selectedMeeting.transcript}
            </div>
          </div>
        {:else}
          <div class="transcript-section card">
            <h3>📝 Transcript</h3>
            <p class="no-content">No transcript available yet.</p>
          </div>
        {/if}

        <!-- Action Items Section -->
        <div class="action-items-section card">
          <h3>📋 Action Items ({selectedMeeting.action_items.length})</h3>
          
          {#if selectedMeeting.action_items.length > 0}
            <div class="action-items-list">
              {#each selectedMeeting.action_items as item}
                <div class="action-item">
                  <div class="item-header">
                    <span class="item-category category-{item.category.toLowerCase()}">
                      {item.category}
                    </span>
                    <span class="item-priority {getPriorityColor(item.priority)}">
                      {item.priority}
                    </span>
                    <span class="item-status status-{item.status.toLowerCase()}">
                      {item.status}
                    </span>
                  </div>
                  
                  <div class="item-content">
                    <p class="item-text">{item.text}</p>
                    {#if item.context}
                      <p class="item-context"><strong>Context:</strong> {item.context}</p>
                    {/if}
                  </div>
                  
                  <div class="item-meta">
                    {#if item.assignee}
                      <span><strong>Assignee:</strong> {item.assignee}</span>
                    {/if}
                    {#if item.due_date}
                      <span><strong>Due:</strong> {formatDate(item.due_date)}</span>
                    {/if}
                    {#if item.timestamp_in_meeting}
                      <span><strong>Time:</strong> {Math.floor(item.timestamp_in_meeting / 60)}:{(item.timestamp_in_meeting % 60).toFixed(0).padStart(2, '0')}</span>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <p class="no-content">No action items extracted yet.</p>
          {/if}
        </div>
      </div>

    {:else}
      <!-- Meetings List -->
      <div class="meetings-list">
        {#if meetings.length > 0}
          <div class="meetings-grid">
            {#each meetings as meeting}
              <div class="meeting-card card">
                <div class="card-header">
                  <h3>{meeting.title}</h3>
                  <span class="meeting-status {getStatusColor(meeting.status)}">
                    {getStatusText(meeting.status)}
                  </span>
                </div>
                
                <div class="card-content">
                  <div class="meeting-info">
                    <p><strong>Started:</strong> {formatDate(meeting.start_time)}</p>
                    {#if meeting.end_time}
                      <p><strong>Ended:</strong> {formatDate(meeting.end_time)}</p>
                    {/if}
                    <p><strong>Duration:</strong> {formatDuration(meeting.duration)}</p>
                    <p><strong>Participants:</strong> {meeting.participants.join(', ') || 'None'}</p>
                  </div>

                  <div class="meeting-stats">
                    <div class="stat">
                      <span class="stat-value">{meeting.action_item_count}</span>
                      <span class="stat-label">Action Items</span>
                    </div>
                    <div class="stat">
                      <span class="stat-value">{meeting.has_transcript ? '✓' : '✗'}</span>
                      <span class="stat-label">Transcript</span>
                    </div>
                  </div>
                </div>

                <div class="card-actions">
                  <button class="btn btn-sm btn-primary" onclick={() => viewMeetingDetails(meeting.id)}>
                    👁️ View Details
                  </button>
                  <button class="btn btn-sm btn-error" onclick={() => deleteMeeting(meeting.id)}>
                    🗑️ Delete
                  </button>
                </div>

                <!-- Processing Status -->
                {#if processingStatuses[meeting.id]}
                  <div class="processing-status">
                    <div class="status-bar">
                      <div class="status-progress" style="width: {processingStatuses[meeting.id].progress * 100}%"></div>
                    </div>
                    <span class="status-text">{processingStatuses[meeting.id].current_step}</span>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {:else}
          <div class="empty-state">
            <h2>📹 No meetings yet</h2>
            <p>Start your first meeting to begin transcribing and extracting action items.</p>
            <button class="btn btn-primary" onclick={() => viewMode = 'new'}>
              🎬 Start Your First Meeting
            </button>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .meetings-page {
    padding: 24px;
    max-width: 1200px;
    margin: 0 auto;
  }

  .page-header {
    margin-bottom: 24px;
  }

  .header-content {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .header-content h1 {
    margin: 0;
    color: var(--text-primary);
    font-size: 1.8rem;
    font-weight: 600;
  }

  .header-actions {
    display: flex;
    gap: 12px;
  }

  /* Current Meeting Status */
  .current-meeting-status {
    padding: 16px;
    border-left: 4px solid var(--accent-primary);
    background: var(--bg-secondary);
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 16px;
  }

  .meeting-info h3 {
    margin: 0;
    color: var(--text-primary);
    font-size: 1.1rem;
  }

  .meeting-details {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    font-size: 0.9rem;
    color: var(--text-secondary);
  }

  .recording-controls {
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
    align-items: center;
  }

  .recording-status {
    font-size: 0.9rem;
    font-weight: 500;
    padding: 6px 12px;
    border-radius: 4px;
    background: var(--bg-tertiary, #1a1a1a);
    border: 1px solid var(--border-primary);
  }

  /* Error and Loading */
  .error-message {
    background: var(--error-bg, rgba(244, 67, 54, 0.1));
    border: 1px solid var(--error, #F44336);
    color: var(--error);
    padding: 12px 16px;
    border-radius: 6px;
    margin-bottom: 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .loading-indicator {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 24px;
    color: var(--text-secondary);
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--border-primary);
    border-top: 2px solid var(--accent-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  /* New Meeting Form */
  .new-meeting-form {
    padding: 24px;
    max-width: 500px;
    margin: 0 auto;
  }

  .new-meeting-form h2 {
    margin: 0 0 24px 0;
    text-align: center;
    color: var(--text-primary);
  }

  .form-group {
    margin-bottom: 20px;
  }

  .form-group label {
    display: block;
    margin-bottom: 6px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .form-input {
    width: 100%;
    padding: 10px 12px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 0.9rem;
    transition: border-color 0.2s;
  }

  .form-input:focus {
    outline: none;
    border-color: var(--accent-primary);
    box-shadow: 0 0 0 2px rgba(74, 144, 226, 0.2);
  }

  .form-actions {
    display: flex;
    gap: 12px;
    justify-content: center;
    margin-top: 24px;
  }

  /* Meeting Detail */
  .meeting-detail {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .meeting-header {
    padding: 24px;
  }

  .meeting-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .meeting-title h2 {
    margin: 0;
    color: var(--text-primary);
  }

  .meeting-id {
    font-size: 0.8rem;
    color: var(--text-secondary);
    font-family: monospace;
  }

  .meeting-meta {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 12px;
    margin-bottom: 16px;
  }

  .meta-item {
    font-size: 0.9rem;
  }

  .meta-item strong {
    color: var(--text-primary);
  }

  .detail-actions {
    display: flex;
    gap: 12px;
  }

  /* Transcript */
  .transcript-section {
    padding: 24px;
  }

  .transcript-section h3 {
    margin: 0 0 16px 0;
    color: var(--text-primary);
  }

  .transcript-content {
    background: var(--bg-tertiary, #1a1a1a);
    padding: 16px;
    border-radius: 6px;
    border: 1px solid var(--border-primary);
    line-height: 1.6;
    white-space: pre-wrap;
    max-height: 400px;
    overflow-y: auto;
  }

  /* Action Items */
  .action-items-section {
    padding: 24px;
  }

  .action-items-section h3 {
    margin: 0 0 16px 0;
    color: var(--text-primary);
  }

  .action-items-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .action-item {
    padding: 16px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-tertiary, #1a1a1a);
  }

  .item-header {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
    flex-wrap: wrap;
  }

  .item-category, .item-priority, .item-status {
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 0.75rem;
    font-weight: 500;
    text-transform: uppercase;
  }

  .item-category {
    background: var(--accent-primary);
    color: white;
  }

  .item-content {
    margin-bottom: 8px;
  }

  .item-text {
    margin: 0 0 8px 0;
    color: var(--text-primary);
  }

  .item-context {
    margin: 0;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .item-meta {
    display: flex;
    gap: 16px;
    font-size: 0.8rem;
    color: var(--text-secondary);
    flex-wrap: wrap;
  }

  /* Meetings List */
  .meetings-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(350px, 1fr));
    gap: 20px;
  }

  .meeting-card {
    padding: 20px;
    transition: transform 0.2s, box-shadow 0.2s;
  }

  .meeting-card:hover {
    transform: translateY(-2px);
    box-shadow: var(--shadow-lg, 0 8px 16px rgba(0, 0, 0, 0.4));
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 12px;
  }

  .card-header h3 {
    margin: 0;
    color: var(--text-primary);
    font-size: 1.1rem;
  }

  .meeting-status {
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 0.75rem;
    font-weight: 500;
    text-transform: uppercase;
  }

  .card-content {
    margin-bottom: 16px;
  }

  .meeting-info p {
    margin: 0 0 4px 0;
    font-size: 0.85rem;
    color: var(--text-secondary);
  }

  .meeting-stats {
    display: flex;
    gap: 20px;
    margin-top: 12px;
  }

  .stat {
    text-align: center;
  }

  .stat-value {
    display: block;
    font-size: 1.2rem;
    font-weight: 600;
    color: var(--accent-primary);
  }

  .stat-label {
    font-size: 0.75rem;
    color: var(--text-secondary);
    text-transform: uppercase;
  }

  .card-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .processing-status {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--border-primary);
  }

  .status-bar {
    width: 100%;
    height: 4px;
    background: var(--border-primary);
    border-radius: 2px;
    overflow: hidden;
    margin-bottom: 6px;
  }

  .status-progress {
    height: 100%;
    background: var(--accent-primary);
    transition: width 0.3s ease;
  }

  .status-text {
    font-size: 0.8rem;
    color: var(--text-secondary);
  }

  /* Empty State */
  .empty-state {
    text-align: center;
    padding: 60px 20px;
  }

  .empty-state h2 {
    margin: 0 0 12px 0;
    color: var(--text-primary);
    font-size: 1.5rem;
  }

  .empty-state p {
    margin: 0 0 24px 0;
    color: var(--text-secondary);
    font-size: 1rem;
  }

  .no-content {
    color: var(--text-secondary);
    font-style: italic;
    text-align: center;
    padding: 24px;
  }

  /* Status Colors */
  .status-success { color: var(--success, #4CAF50) !important; }
  .status-warning { color: var(--warning, #FF9800) !important; }
  .status-error { color: var(--error, #F44336) !important; }
  .status-info { color: var(--info, #2196F3) !important; }

  /* Button Styles */
  .btn {
    padding: 8px 16px;
    border: 1px solid var(--border-primary);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 0.9rem;
    cursor: pointer;
    transition: all 0.2s;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    text-decoration: none;
  }

  .btn:hover:not(:disabled) {
    background: var(--hover-bg);
    border-color: var(--accent-primary);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: var(--accent-tertiary);
    border-color: var(--accent-tertiary);
  }

  .btn-secondary {
    background: transparent;
    border-color: var(--accent-primary);
    color: var(--accent-primary);
  }

  .btn-secondary:hover:not(:disabled) {
    background: var(--accent-primary);
    color: white;
  }

  .btn-warning {
    background: var(--warning);
    border-color: var(--warning);
    color: white;
  }

  .btn-error {
    background: var(--error);
    border-color: var(--error);
    color: white;
  }

  .btn-sm {
    padding: 6px 12px;
    font-size: 0.8rem;
  }

  /* Responsive Design */
  @media (max-width: 768px) {
    .meetings-page {
      padding: 16px;
    }

    .header-content {
      flex-direction: column;
      align-items: stretch;
      gap: 12px;
    }

    .current-meeting-status {
      flex-direction: column;
      align-items: stretch;
    }

    .recording-controls {
      justify-content: center;
    }

    .meetings-grid {
      grid-template-columns: 1fr;
    }

    .meeting-meta {
      grid-template-columns: 1fr;
    }
  }
</style>