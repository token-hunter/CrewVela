import assert from "node:assert/strict";
import test from "node:test";

import {
  PET_ACTIONS,
  clampPetPoint,
  createMovementTarget,
  createPetActionDeck,
  interpolatePetPosition,
  isMovingPetAction,
  PET_ANIMATION_COLUMNS,
  PET_ANIMATION_FRAME_COUNT,
  PET_ANIMATION_ROWS,
  petAnimationFrame,
  petActionIndex,
} from "../src/petBehavior.ts";

test("每只宠物共享完整的 18 种动作协议", () => {
  assert.equal(PET_ACTIONS.length, 18);
  assert.equal(new Set(PET_ACTIONS).size, 18);
  PET_ACTIONS.forEach((action, index) => assert.equal(petActionIndex(action), index));
});

test("每个宠物动作都由三个不同的连续画面组成", () => {
  assert.equal(PET_ANIMATION_COLUMNS, 10);
  assert.equal(PET_ANIMATION_ROWS, 6);
  assert.equal(PET_ANIMATION_FRAME_COUNT, 3);
  for (const action of PET_ACTIONS) {
    const frames = Array.from({ length: PET_ANIMATION_FRAME_COUNT }, (_, frame) => petAnimationFrame(action, frame));
    assert.equal(new Set(frames.map(({ column, row }) => `${column}:${row}`)).size, PET_ANIMATION_FRAME_COUNT);
    assert.ok(frames.every(({ column, row, durationMs }) =>
      column >= 0 && column < PET_ANIMATION_COLUMNS && row >= 0 && row < PET_ANIMATION_ROWS && durationMs >= 90,
    ));
  }
});

test("逐帧播放在第三帧后回到动作第一帧", () => {
  assert.deepEqual(petAnimationFrame("eat", 3), petAnimationFrame("eat", 0));
  assert.deepEqual(petAnimationFrame("run", -1), petAnimationFrame("run", 2));
});

test("一轮自主动作不会遗漏或重复动作", () => {
  const randomValues = [0.02, 0.91, 0.17, 0.73, 0.31, 0.62, 0.44, 0.85];
  let cursor = 0;
  const deck = createPetActionDeck(() => randomValues[cursor++ % randomValues.length]);
  assert.equal(deck.length, 18);
  assert.deepEqual(new Set(deck), new Set(PET_ACTIONS));
});

test("散步、跑步、跳跃和追逐是实际位移动作", () => {
  for (const action of ["walk", "run", "jump", "chase"] as const) {
    assert.equal(isMovingPetAction(action), true);
    const start = { x: 120, y: 280 };
    const target = createMovementTarget(action, start, { width: 1200, height: 800 }, 112, () => 0.8);
    assert.notDeepEqual(target, start);
  }
});

test("跑步在动作中途已经改变屏幕坐标", () => {
  const position = interpolatePetPosition("run", { x: 20, y: 300 }, { x: 800, y: 320 }, 0.5, 112);
  assert.ok(position.x > 20 && position.x < 800);
  assert.ok(position.y > 300 && position.y < 320);
});

test("跳跃同时产生水平位移和垂直抛物线", () => {
  const position = interpolatePetPosition("jump", { x: 100, y: 500 }, { x: 200, y: 500 }, 0.5, 100);
  assert.ok(position.x > 100);
  assert.ok(position.y < 440);
});

test("宠物始终被限制在可见窗口内", () => {
  assert.deepEqual(clampPetPoint({ x: -40, y: 900 }, { width: 1280, height: 720 }, 112), { x: 8, y: 600 });
});
