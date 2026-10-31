import { describe, expect, it } from 'vitest';
import { readDiffAnswer } from './diffAnswer';

describe('the daemon diff answer', () => {
  it('reads the whole-tree answer the daemon serialized', () => {
    const answer = readDiffAnswer({
      type: 'git_diff',
      result: {
        project_id: 'project-1',
        workspace_id: 'workspace-1',
        old_content: '',
        new_content: 'diff --git a/src/lib.rs b/src/lib.rs\n',
        title: 'Repository Diff',
      },
    });
    expect(answer).toEqual({
      title: 'Repository Diff',
      path: null,
      oldContent: '',
      newContent: 'diff --git a/src/lib.rs b/src/lib.rs\n',
    });
  });

  it('reads the per-file answer the daemon serialized', () => {
    const answer = readDiffAnswer({
      type: 'git_diff',
      result: {
        project_id: 'project-1',
        workspace_id: 'workspace-1',
        path: 'src/lib.rs',
        old_content: 'old\n',
        new_content: 'new\n',
        title: 'Diff: src/lib.rs',
      },
    });
    expect(answer).toEqual({
      title: 'Diff: src/lib.rs',
      path: 'src/lib.rs',
      oldContent: 'old\n',
      newContent: 'new\n',
    });
  });

  it('refuses a response that is not a diff answer', () => {
    // Another command's answer must never be drawn as a diff.
    expect(readDiffAnswer({ type: 'git_status', result: { branch: 'main' } })).toBeNull();
    expect(readDiffAnswer(null)).toBeNull();
    expect(readDiffAnswer(undefined)).toBeNull();
    expect(readDiffAnswer('git_diff')).toBeNull();
    expect(readDiffAnswer([])).toBeNull();
  });

  it('refuses a diff answer that is missing its bytes', () => {
    // A half-answer is not an answer: drawing it would claim a diff the
    // daemon never produced.
    expect(readDiffAnswer({ type: 'git_diff', result: { title: 'Repository Diff' } })).toBeNull();
    expect(readDiffAnswer({ type: 'git_diff' })).toBeNull();
    expect(
      readDiffAnswer({
        type: 'git_diff',
        result: { title: 'Repository Diff', old_content: '', new_content: 7 },
      }),
    ).toBeNull();
  });
});
