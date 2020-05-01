// @ts-check

import { Hash, Header, BlockNumber } from '@polkadot/types/interfaces';
import { Callback } from '@polkadot/types/types';

import {
  QueryBlockProducer,
  QueryEventProcessingPack,
  QueryEvent,
  QueryEventBlock,
  ISubstrateQueryService,
} from '.';

const debug = require('debug')('index');

export default class IndexBuilder {
  private _producer: QueryBlockProducer;

  private _processing_pack!: QueryEventProcessingPack;

  private constructor(producer: QueryBlockProducer, processing_pack: QueryEventProcessingPack) {
    this._producer = producer;
    this._processing_pack = processing_pack;
  }

  static create(
    service: ISubstrateQueryService,
    processing_pack: QueryEventProcessingPack
  ): IndexBuilder {
    const producer = new QueryBlockProducer(service);

    return new IndexBuilder(producer, processing_pack);
  }

  async start() {
    // check state

    // STORE THIS SOMEWHERE
    this._producer.on('QueryEventBlock', (query_event_block: QueryEventBlock): void => {
      this._onQueryEventBlock(query_event_block);
    });

    debug('Spawned worker.');

    // open database??

    // Setup worker
    await this._producer.start();

    debug('Started worker.');
  }

  async stop() {}

  _onQueryEventBlock(query_event_block: QueryEventBlock): void {
    console.log(`Yay, block producer at height: #${query_event_block.block_number}`);

    query_event_block.query_events.forEach((query_event, index) => {
      if (!this._processing_pack[query_event.event_method]) {
        console.log(`Unrecognized: ` + query_event.event_name);

        query_event.log(0, debug);
      } else {
        console.log(`Recognized: ` + query_event.event_name);

        // Call event handler to store data on database
        this._processing_pack[query_event.event_method](query_event);
      }
    });
  }
}
