import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { api } from "../api";
import { LocalCommunity } from "./LocalCommunity";
afterEach(() => vi.restoreAllMocks());
it("connects the account here while preserving unrelated workspace settings", async () => {
  vi.spyOn(api, "settings").mockResolvedValue({
    posture: "restricted",
    question_timeout_min: 12,
  });
  const save = vi
    .spyOn(api, "saveSettings")
    .mockResolvedValue({
      posture: "restricted",
      question_timeout_min: 12,
      sculptured_origin: "https://community.example.test",
    });
  const user = userEvent.setup();
  render(
    <MemoryRouter>
      <LocalCommunity />
    </MemoryRouter>,
  );
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Save connection" }),
    ).toBeEnabled(),
  );
  await user.type(
    screen.getByLabelText("Service address"),
    "https://community.example.test",
  );
  await user.click(screen.getByRole("button", { name: "Save connection" }));
  expect(save).toHaveBeenCalledWith({
    posture: "restricted",
    question_timeout_min: 12,
    sculptured_origin: "https://community.example.test",
  });
  expect(
    await screen.findByRole("link", { name: "Open your account →" }),
  ).toHaveAttribute("href", `https://community.example.test/community?workspace=${encodeURIComponent(window.location.origin)}`);
  expect(screen.getByRole("link", { name: "Browse blocks" })).toHaveAttribute("href", `https://community.example.test/community/explore?workspace=${encodeURIComponent(window.location.origin)}`);
  expect(screen.queryByRole("link", { name: "Settings" })).toBeNull();
});

it("once connected, shows the address and changes it only when asked", async () => {
  vi.spyOn(api, "settings").mockResolvedValue({ posture: "restricted", question_timeout_min: 12, sculptured_origin: "https://community.example.test" });
  const user = userEvent.setup();
  render(<MemoryRouter><LocalCommunity /></MemoryRouter>);
  expect(await screen.findByText("community.example.test")).toBeInTheDocument();
  expect(screen.queryByLabelText("Service address")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Change address" }));
  expect(screen.getByLabelText("Service address")).toHaveValue("https://community.example.test");
});
