import { fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it } from 'vitest';
import { AFTER_LOGIN } from '../hosted/afterLogin';
import { SignInPrompt } from './SignInPrompt';

afterEach(() => sessionStorage.clear());

it('signing in from a Community page comes back to that page', () => {
  render(<MemoryRouter initialEntries={['/community/b/review-team?tab=jobs']}><SignInPrompt why="Sign in to save it." /></MemoryRouter>);
  const link = screen.getByRole('link', { name: 'Sign in' });
  link.addEventListener('click', (e) => e.preventDefault());
  fireEvent.click(link);
  expect(sessionStorage.getItem(AFTER_LOGIN)).toBe('/community/b/review-team?tab=jobs');
});
