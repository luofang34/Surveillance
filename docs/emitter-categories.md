# Emitter category normalization

## Purpose

The two ADS-B links do not use the same wire form for emitter category.
`surveillance-aero-link` converts both forms to `EmitterCategory`. The core
uses this type to classify an `AdsbNonIcao` identity.

## 1090 ES

An identification message uses its type code to select a category set. The
three-bit category value has a different meaning in each set.

| Type code | Category values |
| --- | --- |
| 4, set A | 1 light, 2 small, 3 large, 4 high vortex, 5 heavy, 6 high performance, 7 rotorcraft |
| 3, set B | 1 glider, 2 lighter than air, 3 parachutist, 4 ultralight, 6 uncrewed aircraft, 7 space vehicle |
| 2, set C | 1 emergency vehicle, 2 service vehicle, 3 point obstacle, 4 cluster obstacle, 5 line obstacle |
| 1, set D | Reserved |

Zero and reserved values produce no category.

## UAT

UAT uses one flat category code.

| Codes | Meaning |
| --- | --- |
| 1 through 7 | Light through rotorcraft |
| 9 through 12 | Glider through ultralight |
| 14 and 15 | Uncrewed aircraft and space vehicle |
| 17 and 18 | Emergency vehicle and service vehicle |
| 19 through 21 | Point, cluster, and line obstacle |

Codes 0, 8, 13, 16, and 22 through 39 produce no category.

## Core namespace

An aircraft category changes `AdsbNonIcao` to `SelfAssigned`. A vehicle
category changes it to `SurfaceVehicle`. An obstacle category changes it to
`FixedBeacon`. The track keeps its `TrackId`, fields, identity history, and
the initial wire key.

The 1090 ES mapping follows the EASA
[CS-ACNS emitter category table](https://www.easa.europa.eu/en/document-library/easy-access-rules/online-publications/easy-access-rules-airborne-communications?page=40).
The UAT mapping follows the ICAO
[UAT Technical Manual](https://www.icao.int/safety/acp/ACPWGF/ACP-WG-W-1/ACP-WGW01-Report%20on%20A.I.1-Appendix%20C%20-%20UAT%20Tech%20Manual.pdf).
