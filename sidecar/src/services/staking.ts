import Long from "long";
import { ApiService } from "./service.js";
import {
  QueryDelegatorDelegationsResponse,
  QueryDelegatorUnbondingDelegationsResponse,
} from "cosmjs-types/cosmos/staking/v1beta1/query.js";

export class StakingService implements ApiService {
  public delegations(delegatorAddr: string): QueryDelegatorDelegationsResponse {
    return {
      delegationResponses: [],
      pagination: {
        nextKey: new Uint8Array(),
        total: Long.ZERO,
      },
    };
  }

  public unbondingDelegations(
    delegatorAddr: string
  ): QueryDelegatorUnbondingDelegationsResponse {
    return {
      unbondingResponses: [],
      pagination: {
        nextKey: new Uint8Array(),
        total: Long.ZERO,
      },
    };
  }
}
