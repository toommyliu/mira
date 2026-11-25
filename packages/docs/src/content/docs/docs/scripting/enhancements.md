---
title: Enhancements
order: 7
---

Scripts can enhance an item from its shop and equip gear by its enhancement. A selector names the enhancement. For Forge and Awe gear, it also names the special.

## Apply an enhancement

Use an enhancement selector with [`Shops.enhanceItem`](/reference/shops/#enhanceItem) to apply the highest level the player can buy. The call opens the shop and joins its map when needed.

```ts
import { Inventory, Shops } from "mira";

export default function* () {
  const applied = yield* Shops.enhanceItem("Blade", { enhancement: "lucky" });
  if (!applied) return;
  yield* Inventory.equipByEnhancement({ enhancement: "lucky", slot: "weapon" });
}
```

## Select an inventory item

A selector has an `enhancement` name and an optional `special` name. For example, use `{ enhancement: "forge", special: "valiance" }`. [`Inventory.equipByEnhancement`](/reference/inventory/#equipByEnhancement) equips the highest-level matching item already in inventory. A bare `forge` selector needs a `slot` or `special` because Forge uses the same enhancement across slots.

## Supported enhancements

Names ignore case and extra spaces. A curly apostrophe matches a straight one.

### Basic enhancements

`fighter`, `thief`, `hybrid`, `wizard`, `healer`, `spellbreaker`, and `lucky` work in every slot. They have no aliases.

### Weapon specials

Pass a Forge special as the `enhancement`, or as the `special` with `enhancement: "forge"`. An Awe special needs a basic enhancement other than `spellbreaker`, such as `{ enhancement: "lucky", special: "spiral carve" }`.

| Name                | Type  | Aliases                                  |
| ------------------- | ----- | ---------------------------------------- |
| `spiral carve`      | Awe   | `scarve`, `spiral`                       |
| `awe blast`         | Awe   | `ablast`, `aweblast`, `blast`            |
| `health vamp`       | Awe   | `healthvamp`, `hvamp`, `hp vamp`         |
| `mana vamp`         | Awe   | `manavamp`, `mvamp`, `mp vamp`           |
| `powerword die`     | Awe   | `powerword`, `pwd`, `pw die`             |
| `lacerate`          | Forge | `lac`                                    |
| `smite`             | Forge |                                          |
| `valiance`          | Forge | `val`, `vali`                            |
| `arcana's concerto` | Forge | `arcanas`, `arcana concerto`, `concerto` |
| `acheron`           | Forge | `ach`                                    |
| `elysium`           | Forge | `ely`                                    |
| `praxis`            | Forge | `prax`                                   |
| `dauntless`         | Forge | `dtl`                                    |
| `ravenous`          | Forge | `rav`                                    |

Three Forge specials also work with the enhancement that the inventory shows for them: `elysium` with `wizard`, `dauntless` with `fighter`, and `acheron` with `depths`. A bare `forge` selects the Forge enhancement without a special.

### Capes

Pass a cape name as the `enhancement`, or as the `special` with `enhancement: "forge"`.

| Name         | Aliases |
| ------------ | ------- |
| `forge`      |         |
| `absolution` | `abso`  |
| `avarice`    | `ava`   |
| `vainglory`  |         |
| `penitence`  | `peni`  |
| `lament`     | `lam`   |

### Helms

Pass a helm name as the `enhancement`, or as the `special` with `enhancement: "forge"`. You can also pass the name and special that the inventory shows. Helm names have no aliases.

| Name     | Inventory name and special |
| -------- | -------------------------- |
| `forge`  |                            |
| `vim`    | `vim`, `ether`             |
| `examen` | `examen`, `ether`          |
| `pneuma` | `pneuma`, `clairvoyance`   |
| `anima`  | `anima`, `clairvoyance`    |
| `hearty` | `grimskull`, `hearty`      |
