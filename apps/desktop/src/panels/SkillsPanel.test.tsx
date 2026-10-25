import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { SkillsPanel } from './SkillsPanel';
import type { SkillCatalogueView } from '../bridge-types';

const catalogue = (skills: SkillCatalogueView['skills']): SkillCatalogueView => ({
  projectId: 'proj-a',
  skills,
  total: skills.length,
  truncated: false,
});

describe('the desktop skill catalogue surface', () => {
  it('draws the name and the description of every skill', () => {
    render(
      <SkillsPanel
        catalogue={catalogue([
          {
            name: 'testing-ci',
            description: 'Select focused verification.',
            source: 'project',
            contentDigest: 'sha256:abc',
            resourceCount: 2,
          },
        ])}
      />,
    );
    expect(screen.getAllByTestId('skill-item')).toHaveLength(1);
    expect(screen.getByTestId('skill-name')).toHaveTextContent('testing-ci');
    expect(screen.getByTestId('skill-description')).toHaveTextContent(
      'Select focused verification.',
    );
  });

  it('groups the skills by the source the daemon reported', () => {
    render(
      <SkillsPanel
        catalogue={catalogue([
          {
            name: 'testing-ci',
            description: 'Select focused verification.',
            source: 'project',
            contentDigest: 'sha256:abc',
            resourceCount: 0,
          },
          {
            name: 'upgrade',
            description: 'Verified managed-runfile self-upgrade.',
            source: 'global',
            contentDigest: 'sha256:def',
            resourceCount: 1,
          },
        ])}
      />,
    );
    const groups = screen.getAllByTestId('skill-group');
    expect(groups.map((group) => group.getAttribute('data-source'))).toEqual([
      'project',
      'global',
    ]);
    // The source is what tells a reader which of two same-named skills
    // wins, so it stays visible per skill too.
    expect(
      screen.getAllByTestId('skill-item').map((item) => item.getAttribute('data-skill-source')),
    ).toEqual(['project', 'global']);
  });

  it('says the catalogue is unknown until the host has read one', () => {
    render(<SkillsPanel catalogue={null} />);
    // "No skills" and "not read yet" are different claims, and an absent
    // catalogue must not read as an empty one.
    expect(screen.getByTestId('skills-unknown')).toHaveTextContent('Skill catalogue not read yet.');
    expect(screen.queryByTestId('skills-empty')).not.toBeInTheDocument();
  });

  it('says the project has no skills when the host read an empty catalogue', () => {
    render(<SkillsPanel catalogue={catalogue([])} />);
    expect(screen.getByTestId('skills-empty')).toHaveTextContent('No skills discovered.');
    expect(screen.queryByTestId('skills-unknown')).not.toBeInTheDocument();
  });

  it('says the daemon capped the catalogue', () => {
    render(
      <SkillsPanel
        catalogue={{
          projectId: 'proj-a',
          skills: [
            {
              name: 'testing-ci',
              description: 'Select focused verification.',
              source: 'project',
              contentDigest: 'sha256:abc',
              resourceCount: 0,
            },
          ],
          total: 300,
          truncated: true,
        }}
      />,
    );
    // The daemon's bound is part of the answer: the panel must not imply
    // the list is complete.
    expect(screen.getByTestId('skills-truncated')).toHaveTextContent('Showing 1 of 300');
  });

  it('reports a failed read without blanking the catalogue it already has', () => {
    render(
      <SkillsPanel
        catalogue={catalogue([
          {
            name: 'testing-ci',
            description: 'Select focused verification.',
            source: 'project',
            contentDigest: 'sha256:abc',
            resourceCount: 0,
          },
        ])}
        error="stale route"
      />,
    );
    expect(screen.getByTestId('skills-error')).toHaveTextContent(
      'Skill catalogue read failed: stale route',
    );
    expect(screen.getByTestId('skill-name')).toHaveTextContent('testing-ci');
  });

  it('asks the host to read the catalogue again', () => {
    const onRefresh = vi.fn();
    render(<SkillsPanel catalogue={catalogue([])} onRefresh={onRefresh} />);
    fireEvent.click(screen.getByTestId('skills-refresh-button'));
    expect(onRefresh).toHaveBeenCalledTimes(1);
  });
});
